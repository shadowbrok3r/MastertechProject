#pragma once
#include <stdint.h>

// ST7703 720x720 MIPI-DSI + GT911 touch bring-up for the ESP32-P4-WIFI6-Touch-LCD-4B.
int display_init(void);              // 0 on success; panel up
int ui_start(void);                  // 0 on success; LVGL + initial screen
int ui_attach_touch(void);           // 0 on success; GT911 -> LVGL (needs I2C0 installed)
void display_test_bars(void);        // hardware DSI color-bar pattern
void display_backlight(int on);
