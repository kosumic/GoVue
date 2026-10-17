/* Shared browser player, adapted from LiTube (GPL-3.0). See notices/. */
(() => {
  'use strict';
  const video = document.querySelector('video');
  const error = document.querySelector('p');
  const position = Number(video.dataset.start) || 0;
  const fail = () => { error.hidden = false; };
  video.addEventListener('error', fail);
  if (video.dataset.file.endsWith('.mpd')) {
    const player = dashjs.MediaPlayer().create();
    player.updateSettings({debug: {logLevel: 0}, streaming: {buffer: {fastSwitchEnabled: false, bufferToKeep: 20}}});
    player.initialize(video, new URL(video.dataset.file, location.href).href, true);
    player.on(dashjs.MediaPlayer.events.ERROR, fail);
    player.on(dashjs.MediaPlayer.events.STREAM_INITIALIZED, () => { if (position > 0) player.seek(position); });
    window.addEventListener('pagehide', () => player.destroy(), {once: true});
  } else {
    video.src = video.dataset.file;
    video.addEventListener('loadedmetadata', () => { video.currentTime = position; }, {once: true});
  }
})();
