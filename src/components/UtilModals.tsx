import { useEffect, useRef, useState } from "react";
import jsQR from "jsqr";
import { useStore, useActiveTab } from "../store";
import { useT } from "../i18n";
import { confirmDialog, openExternalUrl } from "../backend";
import { compressActive, createNewPdf, insertTableGrid, insertTextAtCenter } from "../actions";
import { getDoc, getPage, docPath } from "../pdf";
import { report } from "../errors";

function Modal({ children, onClose, wide }: { children: React.ReactNode; onClose: () => void; wide?: boolean }) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        onClose();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [onClose]);
  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className={"modal" + (wide ? " modal-wide" : "")} onClick={(e) => e.stopPropagation()}>
        {children}
      </div>
    </div>
  );
}

const PRESETS = [
  { key: "compressHigh" as const, desc: "compressHighDesc" as const, q: 85, dim: 2400 },
  { key: "compressBalanced" as const, desc: "compressBalancedDesc" as const, q: 70, dim: 1600 },
  { key: "compressSmallest" as const, desc: "compressSmallestDesc" as const, q: 55, dim: 1100 },
];

function CompressModal({ onClose }: { onClose: () => void }) {
  const t = useT();
  const tab = useActiveTab();
  const [sel, setSel] = useState(1);
  if (!tab) return null;
  const run = async () => {
    if (tab.dirty && !(await confirmDialog(t("compressDirtyWarn")))) return;
    onClose();
    const p = PRESETS[sel];
    await compressActive(p.q, p.dim);
  };
  return (
    <Modal onClose={onClose}>
      <h3>{t("compressTitle")}</h3>
      <div className="preset-list">
        {PRESETS.map((p, i) => (
          <label key={p.key} className={"preset-row" + (sel === i ? " on" : "")}>
            <input type="radio" name="preset" checked={sel === i} onChange={() => setSel(i)} />
            <span className="preset-meta">
              <span className="preset-name">{t(p.key)}</span>
              <span className="preset-desc">{t(p.desc)}</span>
            </span>
          </label>
        ))}
      </div>
      <div className="modal-actions">
        <button data-tip={t("tipCancelEsc")} onClick={onClose}>{t("btnCancel")}</button>
        <button className="btn-primary" data-tip={t("tipCompressGo")} onClick={run}>{t("btnCompress")}</button>
      </div>
    </Modal>
  );
}

const SIZES = [
  { key: "sizeA4" as const, w: 595.28, h: 841.89 },
  { key: "sizeLetter" as const, w: 612, h: 792 },
  { key: "sizeA5" as const, w: 419.5, h: 595.3 },
  { key: "sizeSquare" as const, w: 600, h: 600 },
  { key: "sizeCustom" as const, w: 0, h: 0 },
];

function NewPdfModal({ onClose }: { onClose: () => void }) {
  const t = useT();
  const [sizeIdx, setSizeIdx] = useState(0);
  const [cw, setCw] = useState(595);
  const [ch, setCh] = useState(842);
  const [pages, setPages] = useState(1);
  const custom = SIZES[sizeIdx].key === "sizeCustom";
  const run = () => {
    const w = custom ? cw : SIZES[sizeIdx].w;
    const h = custom ? ch : SIZES[sizeIdx].h;
    onClose();
    void createNewPdf(w, h, Math.max(1, Math.min(50, pages)));
  };
  return (
    <Modal onClose={onClose}>
      <h3>{t("newPdfTitle")}</h3>
      <div className="form-rows">
        <label className="form-row">
          <span>{t("newSize")}</span>
          <select value={sizeIdx} onChange={(e) => setSizeIdx(Number(e.target.value))} autoFocus>
            {SIZES.map((s, i) => (
              <option key={s.key} value={i}>
                {t(s.key)}
                {s.w > 0 ? ` — ${Math.round(s.w)}×${Math.round(s.h)}` : ""}
              </option>
            ))}
          </select>
        </label>
        {custom && (
          <>
            <label className="form-row">
              <span>{t("newWidth")}</span>
              <input type="number" min={72} max={5000} value={cw} onChange={(e) => setCw(Number(e.target.value) || 72)} />
            </label>
            <label className="form-row">
              <span>{t("newHeight")}</span>
              <input type="number" min={72} max={5000} value={ch} onChange={(e) => setCh(Number(e.target.value) || 72)} />
            </label>
          </>
        )}
        <label className="form-row">
          <span>{t("newPages")}</span>
          <input
            type="number"
            min={1}
            max={50}
            value={pages}
            onChange={(e) => setPages(Number(e.target.value) || 1)}
            onKeyDown={(e) => e.key === "Enter" && run()}
          />
        </label>
      </div>
      <div className="modal-actions">
        <button data-tip={t("tipCancelEsc")} onClick={onClose}>{t("btnCancel")}</button>
        <button className="btn-primary" data-tip={t("tipCreateGo")} onClick={run}>{t("btnCreate")}</button>
      </div>
    </Modal>
  );
}

function TableModal({ onClose }: { onClose: () => void }) {
  const t = useT();
  const [rows, setRows] = useState(3);
  const [cols, setCols] = useState(3);
  const run = () => {
    onClose();
    void insertTableGrid(Math.max(1, Math.min(20, rows)), Math.max(1, Math.min(10, cols)));
  };
  return (
    <Modal onClose={onClose}>
      <h3>{t("tableTitle")}</h3>
      <div className="form-rows">
        <label className="form-row">
          <span>{t("tableRows")}</span>
          <input type="number" min={1} max={20} value={rows} autoFocus
            onChange={(e) => setRows(Number(e.target.value) || 1)}
            onKeyDown={(e) => e.key === "Enter" && run()} />
        </label>
        <label className="form-row">
          <span>{t("tableCols")}</span>
          <input type="number" min={1} max={10} value={cols}
            onChange={(e) => setCols(Number(e.target.value) || 1)}
            onKeyDown={(e) => e.key === "Enter" && run()} />
        </label>
      </div>
      <p className="modal-hint">{t("tableHint")}</p>
      <div className="modal-actions">
        <button data-tip={t("tipCancelEsc")} onClick={onClose}>{t("btnCancel")}</button>
        <button className="btn-primary" data-tip={t("tipInsertGo")} onClick={run}>{t("btnInsert")}</button>
      </div>
    </Modal>
  );
}

function QrModal({ onClose }: { onClose: () => void }) {
  const t = useT();
  const tab = useActiveTab();
  const preset = useRef(useStore.getState().qrPreset).current;
  const [mode, setMode] = useState<"camera" | "page">("camera");
  const [result, setResult] = useState<string | null>(preset);
  const [notice, setNotice] = useState<string | null>(null);
  const videoRef = useRef<HTMLVideoElement>(null);
  const streamRef = useRef<MediaStream | null>(null);

  const stopCamera = () => {
    streamRef.current?.getTracks().forEach((tr) => tr.stop());
    streamRef.current = null;
  };

  useEffect(() => {
    if (mode !== "camera" || result) return;
    let cancelled = false;
    let timer: number | undefined;
    const canvas = document.createElement("canvas");
    (async () => {
      try {
        const stream = await navigator.mediaDevices.getUserMedia({
          video: { facingMode: "environment" },
        });
        if (cancelled) {
          stream.getTracks().forEach((tr) => tr.stop());
          return;
        }
        streamRef.current = stream;
        const video = videoRef.current;
        if (!video) return;
        video.srcObject = stream;
        setNotice(null);
        timer = window.setInterval(() => {
          if (!video.videoWidth) return;
          canvas.width = video.videoWidth;
          canvas.height = video.videoHeight;
          const ctx = canvas.getContext("2d")!;
          ctx.drawImage(video, 0, 0);
          const img = ctx.getImageData(0, 0, canvas.width, canvas.height);
          const code = jsQR(img.data, img.width, img.height);
          if (code?.data) {
            setResult(code.data);
            stopCamera();
          }
        }, 180);
      } catch {
        if (!cancelled) {
          setNotice(t("qrCameraUnavailable"));
          setMode("page");
        }
      }
    })();
    return () => {
      cancelled = true;
      if (timer) window.clearInterval(timer);
      stopCamera();
    };
  }, [mode, result]);

  useEffect(() => () => stopCamera(), []);

  const scanPage = async () => {
    setResult(null);
    if (!tab) {
      setNotice(t("qrNoneOnPage"));
      return;
    }
    const spec = tab.pages[tab.currentPage - 1];
    if (!spec || spec.src === 0) {
      setNotice(t("qrNoneOnPage"));
      return;
    }
    try {
      const entry = await getDoc(tab.id, docPath(tab), tab.docVersion);
      const page = await getPage(entry, spec.src);
      const vp = page.getViewport({ scale: 2, rotation: (page.rotate + spec.rotation + 360) % 360 });
      const canvas = document.createElement("canvas");
      canvas.width = Math.floor(vp.width);
      canvas.height = Math.floor(vp.height);
      await page.render({ canvas, viewport: vp }).promise;
      const ctx = canvas.getContext("2d")!;
      const img = ctx.getImageData(0, 0, canvas.width, canvas.height);
      const code = jsQR(img.data, img.width, img.height);
      if (code?.data) {
        setResult(code.data);
        setNotice(null);
      } else {
        setNotice(t("qrNoneOnPage"));
      }
    } catch (e) {
      report(t("qrNoneOnPage"), e);
    }
  };

  useEffect(() => {
    if (mode === "page" && !result) void scanPage();
  }, [mode]);

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(result ?? "");
      useStore.getState().setStatus(t("stCopied"));
    } catch (e) {
      report(t("stCopyFailed"), e);
    }
  };

  const isLink = !!result && /^https?:\/\//i.test(result);

  return (
    <Modal onClose={onClose} wide>
      <h3>{t("qrTitle")}</h3>
      <div className="qr-tabs">
        <button className={mode === "camera" ? "on" : ""} onClick={() => { setResult(null); setMode("camera"); }}>
          {t("qrCamera")}
        </button>
        <button className={mode === "page" ? "on" : ""} disabled={!tab} onClick={() => { stopCamera(); setResult(null); setMode("page"); }}>
          {t("qrPage")}
        </button>
      </div>
      {notice && <p className="modal-hint">{notice}</p>}
      {!result && mode === "camera" && (
        <>
          <video ref={videoRef} className="qr-video" autoPlay playsInline muted />
          <p className="modal-hint">{t("qrScanning")}</p>
        </>
      )}
      {result && (
        <>
          <pre className="qr-result">{result}</pre>
          <div className="modal-actions qr-actions">
            <button data-tip={t("tipCopyClipboard")} onClick={copy}>{t("btnCopy")}</button>
            {tab && (
              <button
                data-tip={t("tipQrInsert")}
                onClick={() => {
                  insertTextAtCenter(result, 14);
                  onClose();
                }}
              >
                {t("btnInsertAsText")}
              </button>
            )}
            {isLink && (
              <button
                data-tip={t("tipQrOpenLink")}
                onClick={async () => {
                  try {
                    await openExternalUrl(result);
                  } catch {
                    await copy();
                    useStore.getState().setStatus(t("stLinkFailed"));
                  }
                }}
              >
                {t("btnOpenLink")}
              </button>
            )}
            <button
              data-tip={t("tipScanAgain")}
              onClick={() => {
                setResult(null);
                if (mode === "page") void scanPage();
              }}
            >
              {t("btnScanAgain")}
            </button>
          </div>
        </>
      )}
      <div className="modal-actions">
        <button data-tip={t("tipCancelEsc")} onClick={onClose}>{t("btnCancel")}</button>
      </div>
    </Modal>
  );
}

export default function UtilModals() {
  const modal = useStore((s) => s.utilModal);
  const close = () => {
    const st = useStore.getState();
    st.setQrPreset(null);
    st.setUtilModal(null);
  };
  if (!modal) return null;
  if (modal === "compress") return <CompressModal onClose={close} />;
  if (modal === "newpdf") return <NewPdfModal onClose={close} />;
  if (modal === "table") return <TableModal onClose={close} />;
  return <QrModal onClose={close} />;
}
