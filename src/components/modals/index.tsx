import { useStore } from "../../store";
import CompressModal from "./CompressModal";
import NewPdfModal from "./NewPdfModal";
import TableModal from "./TableModal";
import QrModal from "./QrModal";
import ProtectModal from "./ProtectModal";
import UnlockModal from "./UnlockModal";
import SplitModal from "./SplitModal";
import PasswordModal from "./PasswordModal";
import ScanModal from "./ScanModal";
import ExportModal from "./ExportModal";

export default function Modals() {
  const modal = useStore((s) => s.utilModal);
  const close = () => {
    const st = useStore.getState();
    st.setQrPreset(null);
    st.setUtilModal(null);
  };
  return (
    <>
      <PasswordModal />
      {modal === "compress" && <CompressModal onClose={close} />}
      {modal === "newpdf" && <NewPdfModal onClose={close} />}
      {modal === "table" && <TableModal onClose={close} />}
      {modal === "qr" && <QrModal onClose={close} />}
      {modal === "split" && <SplitModal onClose={close} />}
      {modal === "encrypt" && <ProtectModal onClose={close} />}
      {modal === "decrypt" && <UnlockModal onClose={close} />}
      {modal === "scan" && <ScanModal onClose={close} />}
      {modal === "exportDocx" && <ExportModal format="docx" onClose={close} />}
      {modal === "exportXlsx" && <ExportModal format="xlsx" onClose={close} />}
    </>
  );
}
