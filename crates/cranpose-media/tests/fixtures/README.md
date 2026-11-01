# Audio fixtures

`tone.opus` contains a generated 440 Hz tone, 0.12 seconds, 48 kHz stereo.
It was encoded with FFmpeg's libopus encoder and contains no recorded music.
The fixture is shared with Cranamp's Apache-2.0 test suite.

The WAV, FLAC, AIFF, MP2 and ALAC/M4A files contain the same generated tone from Cranamp. `video.webm` pairs a 16 by 16 black VP9 video with Opus audio; it verifies playback of the audio track from a video container. It was made with:

```sh
ffmpeg -f lavfi -i color=black:s=16x16:d=0.12 -i tone.wav -c:v libvpx-vp9 -c:a libopus -shortest video.webm
```

`ac3.mkv`, `eac3.m4a` and `ac3-surround.mkv` contain generated 0.2-second
sine waves encoded with FFmpeg's `ac3` and `eac3` encoders. The surround
fixture has six distinct tones in 5.1 order. They exercise original MKV and
MP4 playback without an intermediate audio file.
