#pragma once
#include <stdint.h>

// ST7703 720x720 MIPI-DSI + GT911 touch bring-up for the ESP32-P4-WIFI6-Touch-LCD-4B.
int display_init(void);              // 0 on success; panel up
int ui_start(void);                  // 0 on success; LVGL + voice screen
int ui_attach_touch(void);           // 0 on success; GT911 -> LVGL (needs audio_init's I2C bus)
void display_test_bars(void);        // hardware DSI color-bar pattern
void display_backlight(int on);

// Voice screen; safe to call from any task.
int ui_ptt_pressed(void);                              // 1 while the talk button is held
void ui_set_status(const char *text, uint32_t color);  // status line, color 0xRRGGBB
void ui_set_transcript(const char *text);              // what the tech said
void ui_set_reply(const char *text);                   // the assistant's reply

// Per-mic visuals: waveform points in -100..100, spectrum bars in 0..100.
#define UI_WAVE_POINTS 64
#define UI_BANDS 16
void ui_viz_update(const int16_t *wave1, const int16_t *wave2, const uint8_t *bars1, const uint8_t *bars2);
