#include "display_shim.h"
#include "audio_shim.h"
#include "driver/gpio.h"
#include "driver/i2c_master.h"
#include "esp_ldo_regulator.h"
#include "esp_lcd_io_i2c.h"
#include "esp_lcd_mipi_dsi.h"
#include "esp_lcd_panel_io.h"
#include "esp_lcd_panel_ops.h"
#include "esp_lcd_st7703.h"
#include "esp_lcd_touch.h"
#include "esp_lcd_touch_gt911.h"
#include "esp_log.h"
#include "esp_lvgl_port.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "lvgl.h"

#define TAG "display_shim"

// MasterTech TUI palette (Deep Pink default): near-black bg, hot-pink accent.
#define COL_BG           0x06060A
#define COL_SURFACE      0x313244
#define COL_TEXT         0xCDD6F4
#define COL_MUTED        0xBAC2DE
#define COL_ACCENT       0xFF1493
#define COL_ACCENT_DIM   0x5A0A36
#define COL_TERTIARY     0xCBA6F7
#define COL_TERTIARY_DIM 0x463A60
#define COL_SUCCESS      0xA6E3A1
#define COL_ERROR        0xF38BA8
#define COL_WARN         0xF9E2AF

#define PIN_TOUCH_RST 23
#define TOUCH_SCL_HZ 400000
#define TOUCH_RETRIES 4
// GT911 firmware start time after a reset.
#define TOUCH_BOOT_MS 200
#ifndef ESP_LCD_TOUCH_IO_I2C_GT911_ADDRESS_BACKUP
#define ESP_LCD_TOUCH_IO_I2C_GT911_ADDRESS_BACKUP 0x14
#endif

#define LCD_H 720
#define LCD_V 720
#define PIN_RST 27
#define PIN_BK 26
#define BK_ON 0
#define LDO_CHAN 3
#define LDO_MV 2500
#define BPP 16

static esp_ldo_channel_handle_t s_ldo;
static esp_lcd_dsi_bus_handle_t s_bus;
static esp_lcd_panel_io_handle_t s_io;
static esp_lcd_panel_handle_t s_panel;

void display_backlight(int on) {
    gpio_set_level(PIN_BK, on ? BK_ON : !BK_ON);
}

int display_init(void) {
    gpio_config_t bk = { .mode = GPIO_MODE_OUTPUT, .pin_bit_mask = 1ULL << PIN_BK };
    if (gpio_config(&bk) != ESP_OK) return -1;
    gpio_set_level(PIN_BK, BK_ON);

    esp_ldo_channel_config_t ldo = { .chan_id = LDO_CHAN, .voltage_mv = LDO_MV };
    if (esp_ldo_acquire_channel(&ldo, &s_ldo) != ESP_OK) return -2;

    esp_lcd_dsi_bus_config_t bus = ST7703_PANEL_BUS_DSI_2CH_CONFIG();
    if (esp_lcd_new_dsi_bus(&bus, &s_bus) != ESP_OK) return -3;

    esp_lcd_dbi_io_config_t dbi = ST7703_PANEL_IO_DBI_CONFIG();
    if (esp_lcd_new_panel_io_dbi(s_bus, &dbi, &s_io) != ESP_OK) return -4;

    esp_lcd_dpi_panel_config_t dpi = ST7703_720_720_PANEL_60HZ_DPI_CONFIG(LCD_COLOR_PIXEL_FORMAT_RGB565);
    st7703_vendor_config_t vc = {
        .flags = { .use_mipi_interface = 1 },
        .mipi_config = { .dsi_bus = s_bus, .dpi_config = &dpi },
    };
    esp_lcd_panel_dev_config_t pc = {
        .reset_gpio_num = PIN_RST,
        .rgb_ele_order = LCD_RGB_ELEMENT_ORDER_RGB,
        .bits_per_pixel = BPP,
        .vendor_config = &vc,
    };
    if (esp_lcd_new_panel_st7703(s_io, &pc, &s_panel) != ESP_OK) return -5;
    if (esp_lcd_panel_reset(s_panel) != ESP_OK) return -6;
    if (esp_lcd_panel_init(s_panel) != ESP_OK) return -7;
    if (esp_lcd_panel_disp_on_off(s_panel, true) != ESP_OK) return -8;
    return 0;
}

void display_test_bars(void) {
    esp_lcd_dpi_panel_set_pattern(s_panel, MIPI_DSI_PATTERN_BAR_VERTICAL);
}

static lv_display_t *s_disp;
static lv_obj_t *s_status;
static lv_obj_t *s_transcript;
static lv_obj_t *s_reply_box;
static lv_obj_t *s_reply;
static lv_obj_t *s_wave[2];
static lv_chart_series_t *s_wave_ser[2];
static lv_obj_t *s_bars[2];
static lv_chart_series_t *s_bars_ser[2];
static lv_obj_t *s_card;
static lv_obj_t *s_card_text;
static lv_obj_t *s_card_hint;
static lv_obj_t *s_card_ok;
static lv_obj_t *s_card_no_label;
static lv_obj_t *s_vol;
static esp_lcd_touch_handle_t s_touch;
static volatile int s_ptt;
static volatile int s_choice = -1;    // approval tap awaiting pickup: 0 deny, 1 approve
static volatile int s_vol_level = -1; // slider value awaiting pickup
static volatile int s_vol_final;      // the slider was released

static void ptt_cb(lv_event_t *e) {
    lv_event_code_t code = lv_event_get_code(e);
    if (code == LV_EVENT_PRESSED) {
        s_ptt = 1;
    } else if (code == LV_EVENT_RELEASED || code == LV_EVENT_PRESS_LOST) {
        s_ptt = 0;
    }
}

static void choice_cb(lv_event_t *e) {
    s_choice = (int)(intptr_t)lv_event_get_user_data(e);
}

static void vol_cb(lv_event_t *e) {
    lv_event_code_t code = lv_event_get_code(e);
    if (code == LV_EVENT_VALUE_CHANGED) {
        s_vol_level = lv_slider_get_value(s_vol);
    } else if (code == LV_EVENT_RELEASED || code == LV_EVENT_PRESS_LOST) {
        s_vol_level = lv_slider_get_value(s_vol);
        s_vol_final = 1;
    }
}

static lv_obj_t *label(lv_obj_t *parent, const lv_font_t *font, uint32_t color, const char *text) {
    lv_obj_t *l = lv_label_create(parent);
    lv_label_set_text(l, text);
    lv_obj_set_style_text_font(l, font, LV_PART_MAIN);
    lv_obj_set_style_text_color(l, lv_color_hex(color), LV_PART_MAIN);
    return l;
}

// Transparent, borderless, unpadded, non-scrolling, touch-transparent.
static void frameless(lv_obj_t *o) {
    lv_obj_set_style_bg_opa(o, LV_OPA_TRANSP, LV_PART_MAIN);
    lv_obj_set_style_border_width(o, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_all(o, 0, LV_PART_MAIN);
    lv_obj_remove_flag(o, LV_OBJ_FLAG_SCROLLABLE);
    lv_obj_remove_flag(o, LV_OBJ_FLAG_CLICKABLE);
}

// One mic's panel: name, live waveform, spectrum bars.
static void mic_panel(lv_obj_t *parent, int idx, const char *name, uint32_t color, uint32_t dim) {
    lv_obj_t *p = lv_obj_create(parent);
    frameless(p);
    lv_obj_set_size(p, 332, 196);
    lv_obj_align(p, idx ? LV_ALIGN_RIGHT_MID : LV_ALIGN_LEFT_MID, 0, 0);
    lv_obj_set_style_bg_color(p, lv_color_hex(COL_SURFACE), LV_PART_MAIN);
    lv_obj_set_style_bg_opa(p, LV_OPA_40, LV_PART_MAIN);
    lv_obj_set_style_radius(p, 14, LV_PART_MAIN);
    lv_obj_set_style_pad_all(p, 10, LV_PART_MAIN);

    lv_obj_t *l = label(p, &lv_font_montserrat_20, color, name);
    lv_obj_align(l, LV_ALIGN_TOP_LEFT, 2, 0);

    lv_obj_t *w = lv_chart_create(p);
    frameless(w);
    lv_obj_set_size(w, 312, 66);
    lv_obj_align(w, LV_ALIGN_TOP_MID, 0, 28);
    lv_chart_set_type(w, LV_CHART_TYPE_LINE);
    lv_chart_set_point_count(w, UI_WAVE_POINTS);
    lv_chart_set_range(w, LV_CHART_AXIS_PRIMARY_Y, -100, 100);
    lv_chart_set_div_line_count(w, 0, 0);
    lv_obj_set_style_line_width(w, 2, LV_PART_ITEMS);
    lv_obj_set_style_width(w, 0, LV_PART_INDICATOR);
    lv_obj_set_style_height(w, 0, LV_PART_INDICATOR);
    s_wave_ser[idx] = lv_chart_add_series(w, lv_color_hex(color), LV_CHART_AXIS_PRIMARY_Y);
    s_wave[idx] = w;

    lv_obj_t *b = lv_chart_create(p);
    frameless(b);
    lv_obj_set_size(b, 312, 80);
    lv_obj_align(b, LV_ALIGN_BOTTOM_MID, 0, 0);
    lv_chart_set_type(b, LV_CHART_TYPE_BAR);
    lv_chart_set_point_count(b, UI_BANDS);
    lv_chart_set_range(b, LV_CHART_AXIS_PRIMARY_Y, 0, 100);
    lv_chart_set_div_line_count(b, 0, 0);
    lv_obj_set_style_pad_column(b, 5, LV_PART_MAIN);
    lv_obj_set_style_radius(b, 3, LV_PART_ITEMS);
    lv_obj_set_style_bg_grad_dir(b, LV_GRAD_DIR_VER, LV_PART_ITEMS);
    lv_obj_set_style_bg_grad_color(b, lv_color_hex(dim), LV_PART_ITEMS);
    s_bars_ser[idx] = lv_chart_add_series(b, lv_color_hex(color), LV_CHART_AXIS_PRIMARY_Y);
    s_bars[idx] = b;
}

// Speaker icon and volume slider in the top-right corner.
static void volume_control(lv_obj_t *scr) {
    lv_obj_t *icon = label(scr, &lv_font_montserrat_26, COL_MUTED, LV_SYMBOL_VOLUME_MAX);
    lv_obj_align(icon, LV_ALIGN_TOP_RIGHT, -232, 36);
    s_vol = lv_slider_create(scr);
    lv_obj_set_size(s_vol, 190, 12);
    lv_obj_align(s_vol, LV_ALIGN_TOP_RIGHT, -28, 44);
    lv_slider_set_range(s_vol, 0, 100);
    lv_slider_set_value(s_vol, 75, LV_ANIM_OFF);
    lv_obj_set_ext_click_area(s_vol, 24);
    lv_obj_set_style_bg_color(s_vol, lv_color_hex(COL_SURFACE), LV_PART_MAIN);
    lv_obj_set_style_bg_opa(s_vol, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_bg_color(s_vol, lv_color_hex(COL_ACCENT), LV_PART_INDICATOR);
    lv_obj_set_style_bg_color(s_vol, lv_color_hex(COL_TEXT), LV_PART_KNOB);
    lv_obj_set_style_pad_all(s_vol, 8, LV_PART_KNOB);
    lv_obj_add_event_cb(s_vol, vol_cb, LV_EVENT_ALL, NULL);
}

static lv_obj_t *card_button(lv_obj_t *card, const char *text, uint32_t fill, uint32_t ink, int choice,
                             lv_obj_t **label_out) {
    lv_obj_t *b = lv_button_create(card);
    lv_obj_set_size(b, 180, 56);
    lv_obj_set_style_radius(b, 28, LV_PART_MAIN);
    lv_obj_set_style_bg_color(b, lv_color_hex(fill), LV_PART_MAIN);
    lv_obj_set_style_shadow_width(b, 0, LV_PART_MAIN);
    lv_obj_t *l = label(b, &lv_font_montserrat_26, ink, text);
    lv_obj_center(l);
    lv_obj_add_event_cb(b, choice_cb, LV_EVENT_CLICKED, (void *)(intptr_t)choice);
    if (label_out) *label_out = l;
    return b;
}

// Approval card over the reply area: request text, hint, Deny and Approve buttons.
static void approval_card(lv_obj_t *scr) {
    s_card = lv_obj_create(scr);
    lv_obj_set_size(s_card, 660, 196);
    lv_obj_align(s_card, LV_ALIGN_TOP_MID, 0, 374);
    lv_obj_remove_flag(s_card, LV_OBJ_FLAG_SCROLLABLE);
    lv_obj_set_style_bg_color(s_card, lv_color_hex(COL_SURFACE), LV_PART_MAIN);
    lv_obj_set_style_bg_opa(s_card, LV_OPA_90, LV_PART_MAIN);
    lv_obj_set_style_border_color(s_card, lv_color_hex(COL_WARN), LV_PART_MAIN);
    lv_obj_set_style_border_width(s_card, 2, LV_PART_MAIN);
    lv_obj_set_style_radius(s_card, 16, LV_PART_MAIN);
    lv_obj_set_style_pad_all(s_card, 14, LV_PART_MAIN);

    lv_obj_t *head = label(s_card, &lv_font_montserrat_20, COL_WARN, "Needs your OK");
    lv_obj_align(head, LV_ALIGN_TOP_LEFT, 0, 0);
    s_card_text = label(s_card, &lv_font_montserrat_26, COL_TEXT, "");
    lv_label_set_long_mode(s_card_text, LV_LABEL_LONG_DOT);
    lv_obj_set_size(s_card_text, 628, 68);
    lv_obj_align(s_card_text, LV_ALIGN_TOP_LEFT, 0, 28);
    s_card_hint = label(s_card, &lv_font_montserrat_20, COL_MUTED, "");
    lv_label_set_long_mode(s_card_hint, LV_LABEL_LONG_WRAP);
    lv_obj_set_width(s_card_hint, 240);
    lv_obj_align(s_card_hint, LV_ALIGN_BOTTOM_LEFT, 0, -8);
    s_card_ok = card_button(s_card, "Approve", COL_SUCCESS, COL_BG, 1, NULL);
    lv_obj_align(s_card_ok, LV_ALIGN_BOTTOM_RIGHT, 0, 0);
    lv_obj_t *no = card_button(s_card, "Deny", COL_SURFACE, COL_ERROR, 0, &s_card_no_label);
    lv_obj_set_style_border_color(no, lv_color_hex(COL_ERROR), LV_PART_MAIN);
    lv_obj_set_style_border_width(no, 2, LV_PART_MAIN);
    lv_obj_align(no, LV_ALIGN_BOTTOM_RIGHT, -196, 0);
    lv_obj_add_flag(s_card, LV_OBJ_FLAG_HIDDEN);
}

int ui_start(void) {
    lvgl_port_cfg_t pcfg = ESP_LVGL_PORT_INIT_CONFIG();
    if (lvgl_port_init(&pcfg) != ESP_OK) return -1;

    lvgl_port_display_cfg_t dcfg = {
        .io_handle = s_io,
        .panel_handle = s_panel,
        .buffer_size = LCD_H * 120,
        .double_buffer = true,
        .hres = LCD_H,
        .vres = LCD_V,
        .color_format = LV_COLOR_FORMAT_RGB565,
        .flags = { .buff_spiram = true },
    };
    lvgl_port_display_dsi_cfg_t dsicfg = { .flags = { .avoid_tearing = false } };
    s_disp = lvgl_port_add_disp_dsi(&dcfg, &dsicfg);
    if (!s_disp) return -2;

    if (!lvgl_port_lock(0)) return -3;
    lv_obj_t *scr = lv_screen_active();
    lv_obj_set_style_bg_color(scr, lv_color_hex(COL_BG), LV_PART_MAIN);
    lv_obj_remove_flag(scr, LV_OBJ_FLAG_SCROLLABLE);

    lv_obj_t *title = label(scr, &lv_font_montserrat_48, COL_ACCENT, "MasterTech");
    lv_obj_align(title, LV_ALIGN_TOP_LEFT, 24, 18);
    volume_control(scr);

    s_status = label(scr, &lv_font_montserrat_26, COL_MUTED, "Starting...");
    lv_obj_align(s_status, LV_ALIGN_TOP_MID, 0, 80);

    lv_obj_t *viz = lv_obj_create(scr);
    frameless(viz);
    lv_obj_set_size(viz, 680, 196);
    lv_obj_align(viz, LV_ALIGN_TOP_MID, 0, 120);
    mic_panel(viz, 0, "MIC 1", COL_ACCENT, COL_ACCENT_DIM);
    mic_panel(viz, 1, "MIC 2", COL_TERTIARY, COL_TERTIARY_DIM);

    s_transcript = label(scr, &lv_font_montserrat_20, COL_MUTED, "");
    lv_label_set_long_mode(s_transcript, LV_LABEL_LONG_DOT);
    lv_obj_set_size(s_transcript, 640, 50);
    lv_obj_set_style_text_align(s_transcript, LV_TEXT_ALIGN_CENTER, LV_PART_MAIN);
    lv_obj_align(s_transcript, LV_ALIGN_TOP_MID, 0, 326);

    s_reply_box = lv_obj_create(scr);
    lv_obj_set_size(s_reply_box, 660, 170);
    lv_obj_align(s_reply_box, LV_ALIGN_TOP_MID, 0, 382);
    lv_obj_set_style_bg_opa(s_reply_box, LV_OPA_TRANSP, LV_PART_MAIN);
    lv_obj_set_style_border_width(s_reply_box, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_all(s_reply_box, 0, LV_PART_MAIN);
    lv_obj_set_scroll_dir(s_reply_box, LV_DIR_VER);

    s_reply = label(s_reply_box, &lv_font_montserrat_26, COL_TEXT, "");
    lv_label_set_long_mode(s_reply, LV_LABEL_LONG_WRAP);
    lv_obj_set_width(s_reply, 640);
    approval_card(scr);

    lv_obj_t *btn = lv_button_create(scr);
    lv_obj_set_size(btn, 420, 112);
    lv_obj_align(btn, LV_ALIGN_BOTTOM_MID, 0, -28);
    lv_obj_set_style_radius(btn, LV_RADIUS_CIRCLE, LV_PART_MAIN);
    lv_obj_set_style_bg_color(btn, lv_color_hex(COL_ACCENT), LV_PART_MAIN);
    lv_obj_set_style_bg_color(btn, lv_color_hex(COL_TERTIARY), LV_PART_MAIN | LV_STATE_PRESSED);
    lv_obj_t *btl = label(btn, &lv_font_montserrat_26, COL_BG, "Hold to talk");
    lv_obj_center(btl);
    lv_obj_add_event_cb(btn, ptt_cb, LV_EVENT_ALL, NULL);

    lvgl_port_unlock();
    return 0;
}

int ui_ptt_pressed(void) {
    return s_ptt;
}

void ui_set_status(const char *text, uint32_t color) {
    if (!s_status || !lvgl_port_lock(0)) return;
    lv_label_set_text(s_status, text);
    lv_obj_set_style_text_color(s_status, lv_color_hex(color), LV_PART_MAIN);
    lvgl_port_unlock();
}

void ui_set_transcript(const char *text) {
    if (!s_transcript || !lvgl_port_lock(0)) return;
    lv_label_set_text(s_transcript, text);
    lvgl_port_unlock();
}

void ui_set_reply(const char *text) {
    if (!s_reply || !lvgl_port_lock(0)) return;
    lv_label_set_text(s_reply, text);
    lv_obj_scroll_to_y(s_reply_box, 0, LV_ANIM_OFF);
    lvgl_port_unlock();
}

void ui_show_approval(const char *text, const char *hint, int can_approve) {
    if (!s_card || !lvgl_port_lock(0)) return;
    lv_label_set_text(s_card_text, text);
    lv_label_set_text(s_card_hint, hint);
    lv_label_set_text(s_card_no_label, can_approve ? "Deny" : "Skip");
    if (can_approve) {
        lv_obj_remove_flag(s_card_ok, LV_OBJ_FLAG_HIDDEN);
    } else {
        lv_obj_add_flag(s_card_ok, LV_OBJ_FLAG_HIDDEN);
    }
    lv_obj_add_flag(s_reply_box, LV_OBJ_FLAG_HIDDEN);
    lv_obj_remove_flag(s_card, LV_OBJ_FLAG_HIDDEN);
    s_choice = -1;
    lvgl_port_unlock();
}

void ui_hide_approval(void) {
    if (!s_card || !lvgl_port_lock(0)) return;
    lv_obj_add_flag(s_card, LV_OBJ_FLAG_HIDDEN);
    lv_obj_remove_flag(s_reply_box, LV_OBJ_FLAG_HIDDEN);
    s_choice = -1;
    lvgl_port_unlock();
}

int ui_approval_choice(void) {
    int c = s_choice;
    s_choice = -1;
    return c;
}

void ui_set_volume(int level) {
    if (!s_vol || !lvgl_port_lock(0)) return;
    lv_slider_set_value(s_vol, level, LV_ANIM_OFF);
    lvgl_port_unlock();
}

int ui_volume_poll(int *final) {
    int v = s_vol_level;
    if (v < 0) return -1;
    s_vol_level = -1;
    *final = s_vol_final;
    s_vol_final = 0;
    return v;
}

void ui_viz_update(const int16_t *wave1, const int16_t *wave2, const uint8_t *bars1, const uint8_t *bars2) {
    if (!s_wave[0] || !lvgl_port_lock(0)) return;
    const int16_t *waves[2] = { wave1, wave2 };
    const uint8_t *bars[2] = { bars1, bars2 };
    for (int m = 0; m < 2; m++) {
        int32_t *wy = lv_chart_get_y_array(s_wave[m], s_wave_ser[m]);
        for (int i = 0; i < UI_WAVE_POINTS; i++) {
            wy[i] = waves[m][i];
        }
        lv_chart_refresh(s_wave[m]);
        int32_t *by = lv_chart_get_y_array(s_bars[m], s_bars_ser[m]);
        for (int i = 0; i < UI_BANDS; i++) {
            by[i] = bars[m][i];
        }
        lv_chart_refresh(s_bars[m]);
    }
    lvgl_port_unlock();
}

// Opens the GT911 at whichever address answers, resetting it through `rst` unless NC.
static int touch_open(i2c_master_bus_handle_t bus, gpio_num_t rst) {
    esp_lcd_panel_io_handle_t tio = NULL;
    esp_lcd_panel_io_i2c_config_t tio_cfg = ESP_LCD_TOUCH_IO_I2C_GT911_CONFIG();
    tio_cfg.scl_speed_hz = TOUCH_SCL_HZ;
    if (i2c_master_probe(bus, ESP_LCD_TOUCH_IO_I2C_GT911_ADDRESS, 100) != ESP_OK &&
        i2c_master_probe(bus, ESP_LCD_TOUCH_IO_I2C_GT911_ADDRESS_BACKUP, 100) == ESP_OK) {
        tio_cfg.dev_addr = ESP_LCD_TOUCH_IO_I2C_GT911_ADDRESS_BACKUP;
    }
    if (esp_lcd_new_panel_io_i2c_v2(bus, &tio_cfg, &tio) != ESP_OK) return -1;
    esp_lcd_touch_config_t tcfg = {
        .x_max = LCD_H,
        .y_max = LCD_V,
        .rst_gpio_num = rst,
        .int_gpio_num = GPIO_NUM_NC,
        .levels = { .reset = 0, .interrupt = 0 },
        .flags = { .swap_xy = 0, .mirror_x = 0, .mirror_y = 0 },
    };
    if (esp_lcd_touch_new_i2c_gt911(tio, &tcfg, &s_touch) != ESP_OK) {
        esp_lcd_panel_io_del(tio);
        return -2;
    }
    return 0;
}

int ui_attach_touch(void) {
    i2c_master_bus_handle_t bus = audio_i2c_bus();
    if (!bus) return -4;
    int rc = touch_open(bus, PIN_TOUCH_RST);
    // Retries after the controller boots: at the latched address, then with a fresh reset.
    for (int attempt = 1; rc == -2 && attempt <= TOUCH_RETRIES; attempt++) {
        ESP_LOGW(TAG, "gt911 init failed; retry %d of %d", attempt, TOUCH_RETRIES);
        vTaskDelay(pdMS_TO_TICKS(TOUCH_BOOT_MS));
        rc = touch_open(bus, attempt % 2 ? GPIO_NUM_NC : PIN_TOUCH_RST);
    }
    if (rc) return rc;
    lvgl_port_touch_cfg_t lt = { .disp = s_disp, .handle = s_touch };
    if (!lvgl_port_add_touch(&lt)) return -3;
    return 0;
}
