#include "display_shim.h"
#include "driver/gpio.h"
#include "esp_ldo_regulator.h"
#include "esp_lcd_mipi_dsi.h"
#include "esp_lcd_panel_io.h"
#include "esp_lcd_panel_ops.h"
#include "esp_lcd_st7703.h"
#include "esp_lcd_touch.h"
#include "esp_lcd_touch_gt911.h"
#include "esp_lvgl_port.h"
#include "lvgl.h"

// MasterTech TUI palette (Deep Pink default): near-black bg, hot-pink accent.
#define COL_BG       0x06060A
#define COL_SURFACE  0x313244
#define COL_TEXT     0xCDD6F4
#define COL_MUTED    0xBAC2DE
#define COL_ACCENT   0xFF1493
#define COL_TERTIARY 0xCBA6F7
#define COL_SUCCESS  0xA6E3A1

#define PIN_TOUCH_RST 23
#define I2C_PORT 0

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
static esp_lcd_touch_handle_t s_touch;
static volatile int s_ptt;

static void ptt_cb(lv_event_t *e) {
    lv_event_code_t code = lv_event_get_code(e);
    if (code == LV_EVENT_PRESSED) {
        s_ptt = 1;
    } else if (code == LV_EVENT_RELEASED || code == LV_EVENT_PRESS_LOST) {
        s_ptt = 0;
    }
}

static lv_obj_t *label(lv_obj_t *parent, const lv_font_t *font, uint32_t color, const char *text) {
    lv_obj_t *l = lv_label_create(parent);
    lv_label_set_text(l, text);
    lv_obj_set_style_text_font(l, font, LV_PART_MAIN);
    lv_obj_set_style_text_color(l, lv_color_hex(color), LV_PART_MAIN);
    return l;
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
    lv_obj_align(title, LV_ALIGN_TOP_MID, 0, 36);

    lv_obj_t *sub = label(scr, &lv_font_montserrat_20, COL_MUTED, "voice console");
    lv_obj_align_to(sub, title, LV_ALIGN_OUT_BOTTOM_MID, 0, 4);

    s_status = label(scr, &lv_font_montserrat_26, COL_MUTED, "Starting...");
    lv_obj_align(s_status, LV_ALIGN_TOP_MID, 0, 140);

    s_transcript = label(scr, &lv_font_montserrat_20, COL_MUTED, "");
    lv_label_set_long_mode(s_transcript, LV_LABEL_LONG_DOT);
    lv_obj_set_size(s_transcript, 640, 50);
    lv_obj_set_style_text_align(s_transcript, LV_TEXT_ALIGN_CENTER, LV_PART_MAIN);
    lv_obj_align(s_transcript, LV_ALIGN_TOP_MID, 0, 186);

    s_reply_box = lv_obj_create(scr);
    lv_obj_set_size(s_reply_box, 660, 290);
    lv_obj_align(s_reply_box, LV_ALIGN_TOP_MID, 0, 244);
    lv_obj_set_style_bg_opa(s_reply_box, LV_OPA_TRANSP, LV_PART_MAIN);
    lv_obj_set_style_border_width(s_reply_box, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_all(s_reply_box, 0, LV_PART_MAIN);
    lv_obj_set_scroll_dir(s_reply_box, LV_DIR_VER);

    s_reply = label(s_reply_box, &lv_font_montserrat_26, COL_TEXT, "");
    lv_label_set_long_mode(s_reply, LV_LABEL_LONG_WRAP);
    lv_obj_set_width(s_reply, 640);

    lv_obj_t *btn = lv_button_create(scr);
    lv_obj_set_size(btn, 420, 120);
    lv_obj_align(btn, LV_ALIGN_BOTTOM_MID, 0, -40);
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

int ui_attach_touch(void) {
    esp_lcd_panel_io_handle_t tio = NULL;
    esp_lcd_panel_io_i2c_config_t tio_cfg = ESP_LCD_TOUCH_IO_I2C_GT911_CONFIG();
    tio_cfg.scl_speed_hz = 0;  // legacy i2c driver rejects a per-device speed
    if (esp_lcd_new_panel_io_i2c_v1(I2C_PORT, &tio_cfg, &tio) != ESP_OK) return -1;
    esp_lcd_touch_config_t tcfg = {
        .x_max = LCD_H,
        .y_max = LCD_V,
        .rst_gpio_num = PIN_TOUCH_RST,
        .int_gpio_num = GPIO_NUM_NC,
        .levels = { .reset = 0, .interrupt = 0 },
        .flags = { .swap_xy = 0, .mirror_x = 0, .mirror_y = 0 },
    };
    if (esp_lcd_touch_new_i2c_gt911(tio, &tcfg, &s_touch) != ESP_OK) return -2;
    lvgl_port_touch_cfg_t lt = { .disp = s_disp, .handle = s_touch };
    if (!lvgl_port_add_touch(&lt)) return -3;
    return 0;
}
