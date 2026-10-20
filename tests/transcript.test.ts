import { test } from 'node:test';
import assert from 'node:assert/strict';
import { currentSegmentIndex, timestamp } from '../src/youtube/transcript.ts';

test('reader formats timestamps including long videos', () => {
  assert.equal(timestamp(3661.9), '1:01:01');
  assert.equal(timestamp(0), '0:00');
});

test('reader follows caption boundaries and seeks without highlighting silence', () => {
  const segments = [
    { start: 2, duration: 3, text: 'First' },
    { start: 5, duration: 1, text: 'Second' },
    { start: 10, duration: 2, text: 'After a gap' },
  ];
  for (const seconds of [null, -1, NaN, Infinity, 0, 1.999, 6, 9.999, 12, 100]) {
    assert.equal(currentSegmentIndex(segments, seconds), -1, `No active line at ${seconds}`);
  }
  for (const [seconds, index] of [[2, 0], [4.999, 0], [5, 1], [10, 2], [5.5, 1], [2, 0]]) {
    assert.equal(currentSegmentIndex(segments, seconds), index);
  }
  assert.equal(currentSegmentIndex([], 0), -1);
});

test('reader prefers the latest overlapping caption and handles missing durations', () => {
  const segments = [
    { start: 0, duration: 10, text: 'Overlapping caption' },
    { start: 3, duration: 0, text: 'Unknown duration' },
    { start: 7, duration: 2, text: 'Same start, first' },
    { start: 7, duration: 3, text: 'Same start, last' },
    { start: 12, duration: 0, text: 'Final caption without duration' },
  ];
  assert.equal(currentSegmentIndex(segments, 2.999), 0);
  assert.equal(currentSegmentIndex(segments, 3), 1);
  assert.equal(currentSegmentIndex(segments, 6.999), 1);
  assert.equal(currentSegmentIndex(segments, 7), 3);
  assert.equal(currentSegmentIndex(segments, 10), -1);
  assert.equal(currentSegmentIndex(segments, 12), 4);
});
