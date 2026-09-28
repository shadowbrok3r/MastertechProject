#pragma once
#include <stddef.h>

// ES8311 + I2S bring-up for the ESP32-P4-WIFI6-Touch-LCD-4B (16 kHz, 16-bit mono).
int audio_init(void);                          // 0 on success
int audio_write(const void *buf, size_t len);  // bytes written to speaker
int audio_read(void *buf, size_t len);         // bytes read from mic
void audio_set_amp(int on);                    // speaker power-amp enable
