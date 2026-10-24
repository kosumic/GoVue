import { useEffect, useState } from 'react';
import { Camera, Clipboard, FileUp, Image as ImageIcon } from 'lucide-react-native';
import { hasChatClipboardAttachment } from '../../modules/govue-core';
import type { AttachmentSource } from '../ai/chatAttachmentSources';
import { PlayerSheet, SheetRow } from '../player/PlayerSheet';

export function ChatAttachmentActions({ visible, onClose, onPick }: { visible: boolean; onClose: () => void; onPick: (source: AttachmentSource) => void }) {
  const [clipboard, setClipboard] = useState(false);
  useEffect(() => {
    if (!visible) return;
    let active = true;
    setClipboard(false);
    void hasChatClipboardAttachment().then(value => { if (active) setClipboard(value); }).catch(() => {});
    return () => { active = false; };
  }, [visible]);
  return <PlayerSheet visible={visible} title="Attach to message" onClose={onClose} presentation="dialog">
    <SheetRow title="Take photo" icon={<Camera />} onPress={() => onPick('camera')} />
    <SheetRow title="Photos" icon={<ImageIcon />} onPress={() => onPick('photo')} />
    <SheetRow title="Files" icon={<FileUp />} detail="Images, PDFs, and text/code files" onPress={() => onPick('file')} />
    {clipboard && <SheetRow title="Paste from clipboard" icon={<Clipboard />} onPress={() => onPick('clipboard')} />}
  </PlayerSheet>;
}
