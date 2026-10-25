# Browser Opus regression fixture

browser-tone.webm contains a 1.3-second, 880 Hz, synthetic oscillator recorded by Chromium 153.0.8010.0 MediaRecorder as audio/webm;codecs=opus (48 kHz, 96 kbit/s). No microphone, user voice or private recording was used.
The previous 132-byte malformed placeholder was replaced. The enabled regression test decodes the complete browser recording and checks duration, finite PCM and audible signal amplitude.
