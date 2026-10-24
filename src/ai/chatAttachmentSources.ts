import { Directory, File, Paths } from 'expo-file-system';
import * as DocumentPicker from 'expo-document-picker';
import * as ImagePicker from 'expo-image-picker';
import { copyChatClipboardAttachment, type ChatAttachmentSource } from '../../modules/govue-core';

export type AttachmentSource = 'camera' | 'photo' | 'file' | 'clipboard';
let sequence = 0;
export function discardChatAttachment(source: ChatAttachmentSource) {
  const cache = `${Paths.cache.uri.replace(/\/$/, '')}/`;
  if (source.uri.startsWith(cache)) {
    try { const file = new File(source.uri); if (file.exists) file.delete(); } catch { /* The OS can remove picker cache files first. */ }
  }
}
export async function pickChatAttachment(source: AttachmentSource): Promise<ChatAttachmentSource[]> {
  if (source === 'clipboard') {
    const file = await copyChatClipboardAttachment();
    if (!file) throw Error('There is no file or image on the clipboard.');
    return [file];
  }
  if (source === 'file') {
    const result = await DocumentPicker.getDocumentAsync({ type: '*/*', multiple: true, copyToCacheDirectory: true });
    return result.canceled ? [] : result.assets.map(file => ({ uri: file.uri, name: file.name, mimeType: file.mimeType }));
  }
  const options: ImagePicker.ImagePickerOptions = { mediaTypes: ['images'], allowsEditing: false, quality: 1,
    preferredAssetRepresentationMode: ImagePicker.UIImagePickerPreferredAssetRepresentationMode.Compatible };
  if (source === 'camera' && !(await ImagePicker.requestCameraPermissionsAsync()).granted) throw Error('Allow camera access in Settings to take a photo.');
  const result = source === 'camera' ? await ImagePicker.launchCameraAsync(options) : await ImagePicker.launchImageLibraryAsync({ ...options, allowsMultipleSelection: true, selectionLimit: 4 });
  return result.canceled ? [] : result.assets.map(file => ({ uri: file.uri, name: file.fileName || 'photo.jpg', mimeType: file.mimeType }));
}
/** Native copies stay on disk, including providers that return content:// URIs. */
export async function stageChatAttachment(source: ChatAttachmentSource, maxBytes: number) {
  const original = new File(source.uri);
  if (!original.exists) throw Error('This file is no longer available.');
  if (original.size > maxBytes) throw Error(`Choose a file smaller than ${maxBytes / 1024 / 1024} MiB.`);
  const directory = new Directory(Paths.cache, `chat-inputs/${Date.now()}-${++sequence}`);
  directory.create({ intermediates: true });
  const staged = new File(directory, 'source');
  try { await original.copy(staged); }
  catch (error) { directory.delete(); throw error; }
  return { source: { ...source, uri: staged.uri }, dispose: () => { if (directory.exists) directory.delete(); } };
}
