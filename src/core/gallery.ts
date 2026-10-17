import {
  coreRequest,
  galleryBytes,
  saveFileToDevice,
} from '../../modules/lingoplay-core';
import * as FileSystem from 'expo-file-system/legacy';
import * as Sharing from 'expo-sharing';
export type GalleryTarget = { urls: string[]; index: number };
export type GalleryImage = {
  uri: string;
  bytes: number;
  extension: string;
  mimeType: string;
  name: string;
};
type Request = {
  url: string;
  sourceUrl: string;
  key: string;
  redirect: number;
  maxBytes: number;
};
type Plan = { request?: Request; image?: GalleryImage };
export const validateGallery = (urls: unknown, index: unknown) =>
  coreRequest<GalleryTarget>({ op: 'gallery_validate', urls, index });
const pending = new Map<string, Promise<GalleryImage>>();
export function loadGalleryImage(url: string): Promise<GalleryImage> {
  const existing = pending.get(url);
  if (existing) return existing;
  const request = fetchGalleryImage(url);
  pending.set(url, request);
  void request.then(
    () => pending.delete(url),
    () => pending.delete(url),
  );
  return request;
}
async function fetchGalleryImage(url: string) {
  let plan = await coreRequest<Plan>({ op: 'gallery_request', url });
  while (plan.request) {
    const request = plan.request;
    const response = await galleryBytes(request.url, request.key);
    plan = await coreRequest<Plan>({
      op: 'gallery_response',
      request,
      response,
    });
  }
  if (!plan.image) throw Error('Image could not load.');
  return plan.image;
}
export async function exportGalleryImage(image: GalleryImage, save: boolean) {
  if (save) {
    const result = await saveFileToDevice(
      image.uri,
      image.name,
      image.mimeType,
    );
    if (result !== null) return;
  }
  if (!(await Sharing.isAvailableAsync()))
    throw Error('File sharing is unavailable.');
  const folder = `${FileSystem.cacheDirectory}gallery-exports/`;
  await FileSystem.makeDirectoryAsync(folder, { intermediates: true });
  const uri = folder + encodeURIComponent(image.name);
  await FileSystem.copyAsync({ from: image.uri, to: uri });
  await Sharing.shareAsync(uri, {
    mimeType: image.mimeType,
    dialogTitle: save ? 'Save image' : 'Share image',
  });
}
