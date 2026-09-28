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
static esp_lcd_touch_handle_t s_touch;

static void tap_cb(lv_event_t *e) {
    (void)e;
    static int n;
    if (s_status) {
        n++;
        lv_label_set_text_fmt(s_status, "tap %d", n);
    }
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

    lv_obj_t *title = lv_label_create(scr);
    lv_label_set_text(title, "MasterTech");
    lv_obj_set_style_text_color(title, lv_color_hex(COL_ACCENT), LV_PART_MAIN);
    lv_obj_set_style_text_font(title, &lv_font_montserrat_48, LV_PART_MAIN);
    lv_obj_align(title, LV_ALIGN_TOP_MID, 0, 60);

    lv_obj_t *sub = lv_label_create(scr);
    lv_label_set_text(sub, "voice console");
    lv_obj_set_style_text_color(sub, lv_color_hex(COL_MUTED), LV_PART_MAIN);
    lv_obj_set_style_text_font(sub, &lv_font_montserrat_20, LV_PART_MAIN);
    lv_obj_align_to(sub, title, LV_ALIGN_OUT_BOTTOM_MID, 0, 8);

    s_status = lv_label_create(scr);
    lv_label_set_text(s_status, "idle");
    lv_obj_set_style_text_color(s_status, lv_color_hex(COL_SUCCESS), LV_PART_MAIN);
    lv_obj_set_style_text_font(s_status, &lv_font_montserrat_26, LV_PART_MAIN);
    lv_obj_center(s_status);

    lv_obj_t *btn = lv_button_create(scr);
    lv_obj_set_size(btn, 260, 96);
    lv_obj_align(btn, LV_ALIGN_CENTER, 0, 140);
    lv_obj_set_style_bg_color(btn, lv_color_hex(COL_ACCENT), LV_PART_MAIN);
    lv_obj_set_style_radius(btn, 16, LV_PART_MAIN);
    lv_obj_t *btl = lv_label_create(btn);
    lv_label_set_text(btl, "Tap to test");
    lv_obj_set_style_text_color(btl, lv_color_hex(COL_BG), LV_PART_MAIN);
    lv_obj_set_style_text_font(btl, &lv_font_montserrat_26, LV_PART_MAIN);
    lv_obj_center(btl);
    lv_obj_add_event_cb(btn, tap_cb, LV_EVENT_CLICKED, NULL);

    lvgl_port_unlock();
    return 0;
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
