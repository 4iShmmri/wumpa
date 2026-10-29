export interface QrResult {
  data: string;
  corners: [number, number][]; // in the pixels of the image that was scanned
}

let worker: Worker | null = null;
let nextId = 1;
const pending = new Map<number, (r: QrResult | null) => void>();

function ensureWorker(): Worker {
  if (worker) return worker;
  worker = new Worker(new URL("./qrWorker.ts", import.meta.url), { type: "module" });
  worker.onmessage = (e: MessageEvent<{ id: number; result: QrResult | null }>) => {
    pending.get(e.data.id)?.(e.data.result);
    pending.delete(e.data.id);
  };
  worker.onerror = () => {
    for (const resolve of pending.values()) resolve(null);
    pending.clear();
  };
  return worker;
}

// Scans an image for a QR code in a worker; the pixel buffer is transferred, not copied.
export function scanQr(img: ImageData): Promise<QrResult | null> {
  return new Promise((resolve) => {
    const id = nextId++;
    pending.set(id, resolve);
    ensureWorker().postMessage({ id, data: img.data, width: img.width, height: img.height }, [
      img.data.buffer,
    ]);
  });
}
