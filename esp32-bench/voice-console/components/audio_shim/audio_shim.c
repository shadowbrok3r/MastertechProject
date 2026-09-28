#include "audio_shim.h"
#include "driver/i2c.h"
#include "driver/i2s_std.h"
#include "driver/gpio.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "es8311.h"

#define I2C_PORT 0
#define I2C_SDA 7
#define I2C_SCL 8
#define PA_GPIO 53
#define I2S_PORT 0
#define I2S_MCK 13
#define I2S_BCK 12
#define I2S_WS 10
#define I2S_DO 9
#define I2S_DI 11
#define SAMPLE_RATE 16000
#define MCLK_MULT 384

static i2s_chan_handle_t s_tx;
static i2s_chan_handle_t s_rx;

static int i2c_setup(void) {
    i2c_config_t c = {
        .mode = I2C_MODE_MASTER,
        .sda_io_num = I2C_SDA,
        .scl_io_num = I2C_SCL,
        .sda_pullup_en = GPIO_PULLUP_ENABLE,
        .scl_pullup_en = GPIO_PULLUP_ENABLE,
        .master.clk_speed = 100000,
    };
    if (i2c_param_config(I2C_PORT, &c) != ESP_OK) return -1;
    return i2c_driver_install(I2C_PORT, I2C_MODE_MASTER, 0, 0, 0) == ESP_OK ? 0 : -1;
}

static int i2s_setup(void) {
    i2s_chan_config_t cc = I2S_CHANNEL_DEFAULT_CONFIG(I2S_PORT, I2S_ROLE_MASTER);
    if (i2s_new_channel(&cc, &s_tx, &s_rx) != ESP_OK) return -1;
    i2s_std_config_t std = {
        .clk_cfg = I2S_STD_CLK_DEFAULT_CONFIG(SAMPLE_RATE),
        .slot_cfg = I2S_STD_PHILIPS_SLOT_DEFAULT_CONFIG(I2S_DATA_BIT_WIDTH_16BIT, I2S_SLOT_MODE_MONO),
        .gpio_cfg = {
            .mclk = I2S_MCK,
            .bclk = I2S_BCK,
            .ws = I2S_WS,
            .dout = I2S_DO,
            .din = I2S_DI,
            .invert_flags = { .mclk_inv = false, .bclk_inv = false, .ws_inv = false },
        },
    };
    std.clk_cfg.mclk_multiple = I2S_MCLK_MULTIPLE_384;
    if (i2s_channel_init_std_mode(s_tx, &std) != ESP_OK) return -2;
    if (i2s_channel_init_std_mode(s_rx, &std) != ESP_OK) return -3;
    if (i2s_channel_enable(s_tx) != ESP_OK) return -4;
    if (i2s_channel_enable(s_rx) != ESP_OK) return -5;
    return 0;
}

static int es8311_setup(void) {
    es8311_handle_t h = es8311_create(I2C_PORT, ES8311_ADDRRES_0);
    if (!h) return -1;
    es8311_clock_config_t clk = {
        .mclk_inverted = false,
        .sclk_inverted = false,
        .mclk_from_mclk_pin = true,
        .mclk_frequency = SAMPLE_RATE * MCLK_MULT,
        .sample_frequency = SAMPLE_RATE,
    };
    if (es8311_init(h, &clk, ES8311_RESOLUTION_16, ES8311_RESOLUTION_16) != ESP_OK) return -2;
    if (es8311_sample_frequency_config(h, SAMPLE_RATE * MCLK_MULT, SAMPLE_RATE) != ESP_OK) return -3;
    if (es8311_voice_volume_set(h, 75, NULL) != ESP_OK) return -4;
    if (es8311_microphone_config(h, false) != ESP_OK) return -5;
    return 0;
}

void audio_set_amp(int on) {
    gpio_set_level(PA_GPIO, on ? 1 : 0);
}

int audio_init(void) {
    gpio_config_t io = { .mode = GPIO_MODE_OUTPUT, .pin_bit_mask = 1ULL << PA_GPIO };
    gpio_config(&io);
    gpio_set_level(PA_GPIO, 1);
    if (i2c_setup()) return -1;
    if (i2s_setup()) return -2;
    if (es8311_setup()) return -3;
    return 0;
}

int audio_write(const void *buf, size_t len) {
    size_t written = 0;
    i2s_channel_write(s_tx, buf, len, &written, 1000 / portTICK_PERIOD_MS);
    return (int)written;
}

int audio_read(void *buf, size_t len) {
    size_t got = 0;
    i2s_channel_read(s_rx, buf, len, &got, 1000 / portTICK_PERIOD_MS);
    return (int)got;
}
