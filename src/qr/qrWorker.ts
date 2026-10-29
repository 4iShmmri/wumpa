// Decodes QR codes off the main thread so page rendering and scrolling stay smooth.
import jsQR from "jsqr";

interface Request {
  id: number;
  data: Uint8ClampedArray;
  width: number;
  height: number;
}

self.onmessage = (e: MessageEvent<Request>) => {
  const { id, data, width, height } = e.data;
  const code = jsQR(data, width, height);
  const result = code?.data
    ? {
        data: code.data,
        corners: [
          code.location.topLeftCorner,
          code.location.topRightCorner,
          code.location.bottomRightCorner,
          code.location.bottomLeftCorner,
        ].map((p) => [p.x, p.y] as [number, number]),
      }
    : null;
  (self as unknown as Worker).postMessage({ id, result });
};
