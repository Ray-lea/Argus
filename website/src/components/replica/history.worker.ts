import { paint, type RasterRequest } from "./historyPainter";
const surface = new OffscreenCanvas(1, 1);
const ctx = surface.getContext("2d")!;
self.onmessage = ({ data: r }: MessageEvent<RasterRequest>) => {
  if (surface.width !== r.width) surface.width = r.width;
  if (surface.height !== r.height) surface.height = r.height;
  paint(ctx, r);
  const image = surface.transferToImageBitmap();
  (self as unknown as { postMessage: (value: unknown, transfer: Transferable[]) => void }).postMessage(
    { id: r.id, revision: r.revision, image },
    [image],
  );
};
