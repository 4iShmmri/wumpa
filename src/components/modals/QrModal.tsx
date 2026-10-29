import jsQR from "jsqr";
import { useEffect, useRef, useState } from "react";
import Modal from "../ui/Modal";
import { useStore, useActiveTab } from "../../store";
import { useT } from "../../i18n";
import { openExternalUrl } from "../../backend";
import { insertTextAtCenter } from "../../actions";
import { getDoc, getPage, docPath } from "../../pdf";
import { report } from "../../errors";

export default function QrModal({ onClose }: { onClose: () => void }) {
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
    if (!spec || spec.src <= 0) {
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
        <button
          className={mode === "camera" ? "on" : ""}
          onClick={() => {
            setResult(null);
            setMode("camera");
          }}
        >
          {t("qrCamera")}
        </button>
        <button
          className={mode === "page" ? "on" : ""}
          disabled={!tab}
          onClick={() => {
            stopCamera();
            setResult(null);
            setMode("page");
          }}
        >
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
            <button data-tip={t("tipCopyClipboard")} onClick={copy}>
              {t("btnCopy")}
            </button>
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
        <button data-tip={t("tipCancelEsc")} onClick={onClose}>
          {t("btnCancel")}
        </button>
      </div>
    </Modal>
  );
}

// ---------- password protection ----------

// 0..4: length and character variety, with a floor for very short passwords.
