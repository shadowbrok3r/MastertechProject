#pragma once
#include <stddef.h>
#include <stdint.h>

// ES8311 speaker and ES7210 dual-mic (TDM) audio at 16 kHz 16-bit via esp_codec_dev.
int audio_init(void);                          // 0 on success
void *audio_i2c_bus(void);                     // the i2c_master bus handle, for the touch panel
int audio_write(const void *buf, size_t len);  // bytes written to speaker
void audio_set_amp(int on);                    // speaker power-amp enable

// Mono mic PCM (MIC1) for streaming; only queued while capture is on.
void audio_capture(int on);                    // start (draining stale audio) or stop queueing
int audio_read(void *buf, size_t len);         // bytes read from the mic stream

// The latest `n` samples of each mic (n <= 1024), oldest first.
void audio_tap_latest(int16_t *mic1, int16_t *mic2, size_t n);

// Wake word and voice activity from esp-sr's AFE (two mics plus the speaker reference).
int audio_sr_init(void);     // 0 on success; needs the "model" partition
int audio_wake_take(void);   // 1 once per detected wake word
// Pops the oldest 32 ms frame: MIC1+MIC2 level in dBFS and the VAD state; 0 when none is queued.
int audio_level_frame(float *db, int *speech);

void audio_set_volume(int level);  // speaker volume, 0..100

// Streamed reply playback (PCM16LE 16 kHz mono) through a PSRAM buffer.
void audio_play_begin(void);                       // start accepting a reply's PCM
int audio_play_push(const void *buf, size_t len);  // bytes queued
void audio_play_end(void);                         // no more PCM; play out the queue
void audio_play_stop(void);                        // drop queued PCM now
int audio_play_active(void);                       // 1 while a reply is queued or playing
