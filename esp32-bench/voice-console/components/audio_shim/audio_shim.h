#pragma once
#include <stddef.h>

// ES8311 + I2S bring-up for the ESP32-P4-WIFI6-Touch-LCD-4B (16 kHz, 16-bit mono).
int audio_init(void);                          // 0 on success
int audio_write(const void *buf, size_t len);  // bytes written to speaker
int audio_read(void *buf, size_t len);         // bytes read from mic
void audio_set_amp(int on);                    // speaker power-amp enable

// Streamed reply playback (PCM16LE 16 kHz mono) through a PSRAM buffer.
void audio_play_begin(void);                       // start accepting a reply's PCM
int audio_play_push(const void *buf, size_t len);  // bytes queued
void audio_play_end(void);                         // no more PCM; play out the queue
void audio_play_stop(void);                        // drop queued PCM now
int audio_play_active(void);                       // 1 while a reply is queued or playing
