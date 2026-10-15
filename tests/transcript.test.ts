import { test } from 'node:test';
import assert from 'node:assert/strict';
import { browserAddress, youtubeVideoId } from '../src/browser/address.ts';
import { timestamp } from '../src/youtube/transcript.ts';

const videoId = 'abcdefghijk';
const origin = `https://www.youtube.com/watch?v=${videoId}`;

test('address input distinguishes websites, searches, unsafe schemes, and short YouTube links', () => {
  assert.equal(browserAddress('example.com'), 'https://example.com/');
  assert.equal(browserAddress('http://localhost:8080/test'), 'http://localhost:8080/test');
  assert.equal(browserAddress('localhost:8080'), 'https://localhost:8080/');
  assert.equal(browserAddress('learn Japanese'), 'https://www.google.com/search?q=learn%20Japanese');
  assert.equal(browserAddress(`https://youtu.be/${videoId}?t=12&si=tracking`), `${origin}&t=12`);
  for (const input of ['javascript:alert(1)', 'file:///etc/passwd', 'https://user:pass@example.com', '']) assert.throws(() => browserAddress(input));
});

test('only real HTTPS YouTube video routes enable extraction', () => {
  for (const url of [origin, `https://m.youtube.com/shorts/${videoId}`, `https://www.youtube.com/live/${videoId}`]) assert.equal(youtubeVideoId(url), videoId);
  for (const url of ['https://www.youtube.com/', origin.replace('youtube.com', 'youtube.com.evil.test'), origin.replace('https:', 'http:'), 'https://example.com/watch?v=abcdefghijk', 'https://youtube.com/watch?v=bad']) assert.equal(youtubeVideoId(url), null);
});

test('reader formats timestamps including long videos', () => {
  assert.equal(timestamp(3661.9), '1:01:01');
  assert.equal(timestamp(0), '0:00');
});
