#include "audio_shim.h"
#include <inttypes.h>
#include <math.h>
#include <string.h>
#include "driver/i2c_master.h"
#include "driver/i2s_std.h"
#include "driver/i2s_tdm.h"
#include "driver/gpio.h"
#include "esp_afe_sr_models.h"
#include "esp_attr.h"
#include "esp_codec_dev.h"
#include "esp_codec_dev_defaults.h"
#include "esp_err.h"
#include "esp_heap_caps.h"
#include "esp_log.h"
#include "freertos/FreeRTOS.h"
#include "freertos/queue.h"
#include "freertos/stream_buffer.h"
#include "freertos/task.h"

#define TAG "audio_shim"

#define I2C_PORT I2C_NUM_1
#define I2C_SDA 7
#define I2C_SCL 8
#define PA_GPIO 53
#define I2S_MCK 13
#define I2S_BCK 12
#define I2S_WS 10
#define I2S_DO 9
#define I2S_DI 11
#define SAMPLE_RATE 16000
#define OUT_VOLUME 75
#define MIC_GAIN_DB 30.0f
#define REF_GAIN_DB 18.0f

// ES7210 TDM slot order is MIC1, MIC3, MIC2, MIC4; MIC3 carries the speaker reference.
#define TDM_SLOTS 4
#define SLOT_MIC1 0
#define SLOT_REF 1
#define SLOT_MIC2 2
// AFE input: two mics, then the playback reference.
#define AFE_FORMAT "MMR"
#define AFE_CHANNELS 3
// Silence that ends a VAD speech run.
#define VAD_END_MS 700
// 16 ms of 16 kHz frames per read.
#define CAP_FRAMES 256
#define CAP_BYTES (CAP_FRAMES * TDM_SLOTS * sizeof(int16_t))
// Per-mic tap length; a power of two.
#define TAP_LEN 1024
#define MIC_BUF_BYTES (64 * 1024)
#define MIC_CHUNK 2048
#define SLOT_REPORTS 30
// 32 ms mic level frames, 2 s of them queued.
#define LEVEL_FRAMES 512
#define LEVEL_QUEUE 64

// ~32 s of 16 kHz mono PCM16, held in PSRAM.
#define PLAY_BUF_BYTES (1024 * 1024)
#define PLAY_CHUNK 2048

static i2c_master_bus_handle_t s_i2c;
static i2s_chan_handle_t s_tx;
static i2s_chan_handle_t s_rx;
static esp_codec_dev_handle_t s_out;
static esp_codec_dev_handle_t s_in;

// Heap-allocated in buffers_setup.
static int16_t *s_tap1;
static int16_t *s_tap2;
static int16_t *s_cap;   // one read of TDM frames
static int16_t *s_mono;  // MIC1 of that read
static uint8_t *s_chunk; // one speaker write
static uint32_t s_tap_pos;
static portMUX_TYPE s_tap_lock = portMUX_INITIALIZER_UNLOCKED;

static StreamBufferHandle_t s_mic;
static StaticStreamBuffer_t s_mic_ctl;
static volatile int s_capture;  // mic PCM is queued for audio_read

static StreamBufferHandle_t s_play;
static StaticStreamBuffer_t s_play_ctl;
static volatile int s_accept;  // pushes are queued
static volatile int s_eos;     // the current reply has no more data
static volatile int s_flush;   // queued audio is to be dropped
static volatile int s_busy;    // a reply is queued or playing

// Occupies TCM so no heap allocation, task stacks included, lands there.
static TCM_DRAM_ATTR volatile uint8_t s_tcm_fill[0x1F80];

static const esp_afe_sr_iface_t *s_afe;
static esp_afe_sr_data_t *s_afe_data;
static int16_t *s_feed;        // interleaved AFE input frames
static int s_feed_frames;      // frames per AFE feed
static int s_feed_fill;
static volatile int s_wake;    // wake word heard; read once by audio_wake_take
static volatile int s_speech;  // VAD reports speech

typedef struct {
    float db;
    int32_t speech;
} level_frame_t;

static QueueHandle_t s_levels;

// Guard words around each shim buffer to catch overruns.
#define GUARD_WORDS 4
#define GUARD_VALUE 0xA5C3E1F7u
#define MAX_GUARDED 8

typedef struct {
    const char *name;
    uint32_t *head;
    size_t words;  // payload words between the guards
} guarded_t;

static guarded_t s_guarded[MAX_GUARDED];
static int s_guarded_count;

// Allocates `bytes` with guard words on both sides and records it for audio_guard_check.
static void *guarded_alloc(const char *name, size_t bytes, uint32_t caps) {
    size_t words = (bytes + 3) / 4;
    uint32_t *head = heap_caps_malloc((words + 2 * GUARD_WORDS) * 4, caps);
    if (!head || s_guarded_count == MAX_GUARDED) return NULL;
    for (int i = 0; i < GUARD_WORDS; i++) {
        head[i] = GUARD_VALUE;
        head[GUARD_WORDS + words + i] = GUARD_VALUE;
    }
    memset(head + GUARD_WORDS, 0, words * 4);
    s_guarded[s_guarded_count++] = (guarded_t){ .name = name, .head = head, .words = words };
    ESP_LOGI(TAG, "buffer %s at %p, %u B", name, (void *)(head + GUARD_WORDS), (unsigned)bytes);
    return head + GUARD_WORDS;
}

int audio_guard_check(void) {
    int broken = 0;
    for (int b = 0; b < s_guarded_count; b++) {
        const guarded_t *g = &s_guarded[b];
        for (int i = 0; i < GUARD_WORDS; i++) {
            uint32_t before = g->head[i];
            uint32_t after = g->head[GUARD_WORDS + g->words + i];
            if (before != GUARD_VALUE || after != GUARD_VALUE) {
                ESP_LOGE(TAG, "guard broken on %s word %d: before 0x%08" PRIx32 ", after 0x%08" PRIx32,
                         g->name, i, before, after);
                broken++;
            }
        }
    }
    return broken;
}

static int i2c_setup(void) {
    i2c_master_bus_config_t cfg = {
        .i2c_port = I2C_PORT,
        .sda_io_num = I2C_SDA,
        .scl_io_num = I2C_SCL,
        .clk_source = I2C_CLK_SRC_DEFAULT,
        .glitch_ignore_cnt = 7,
        .flags = { .enable_internal_pullup = 1 },
    };
    return i2c_new_master_bus(&cfg, &s_i2c) == ESP_OK ? 0 : -1;
}

// Duplex I2S0: STD TX to the ES8311, TDM RX from the ES7210.
static int i2s_setup(void) {
    i2s_chan_config_t cc = {
        .id = I2S_NUM_0,
        .role = I2S_ROLE_MASTER,
        .dma_desc_num = 6,
        .dma_frame_num = 240,
        .auto_clear_after_cb = true,
        .auto_clear_before_cb = false,
        .intr_priority = 0,
    };
    if (i2s_new_channel(&cc, &s_tx, &s_rx) != ESP_OK) return -1;
    i2s_std_config_t std = {
        .clk_cfg = {
            .sample_rate_hz = SAMPLE_RATE,
            .clk_src = I2S_CLK_SRC_DEFAULT,
            .ext_clk_freq_hz = 0,
            .mclk_multiple = I2S_MCLK_MULTIPLE_256,
        },
        .slot_cfg = {
            .data_bit_width = I2S_DATA_BIT_WIDTH_16BIT,
            .slot_bit_width = I2S_SLOT_BIT_WIDTH_AUTO,
            .slot_mode = I2S_SLOT_MODE_STEREO,
            .slot_mask = I2S_STD_SLOT_BOTH,
            .ws_width = I2S_DATA_BIT_WIDTH_16BIT,
            .ws_pol = false,
            .bit_shift = true,
            .left_align = true,
            .big_endian = false,
            .bit_order_lsb = false,
        },
        .gpio_cfg = {
            .mclk = I2S_MCK,
            .bclk = I2S_BCK,
            .ws = I2S_WS,
            .dout = I2S_DO,
            .din = I2S_GPIO_UNUSED,
            .invert_flags = { .mclk_inv = false, .bclk_inv = false, .ws_inv = false },
        },
    };
    i2s_tdm_config_t tdm = {
        .clk_cfg = {
            .sample_rate_hz = SAMPLE_RATE,
            .clk_src = I2S_CLK_SRC_DEFAULT,
            .ext_clk_freq_hz = 0,
            .mclk_multiple = I2S_MCLK_MULTIPLE_256,
            .bclk_div = 8,
        },
        .slot_cfg = {
            .data_bit_width = I2S_DATA_BIT_WIDTH_16BIT,
            .slot_bit_width = I2S_SLOT_BIT_WIDTH_AUTO,
            .slot_mode = I2S_SLOT_MODE_STEREO,
            .slot_mask = I2S_TDM_SLOT0 | I2S_TDM_SLOT1 | I2S_TDM_SLOT2 | I2S_TDM_SLOT3,
            .ws_width = I2S_TDM_AUTO_WS_WIDTH,
            .ws_pol = false,
            .bit_shift = true,
            .left_align = false,
            .big_endian = false,
            .bit_order_lsb = false,
            .skip_mask = false,
            .total_slot = I2S_TDM_AUTO_SLOT_NUM,
        },
        .gpio_cfg = {
            .mclk = I2S_MCK,
            .bclk = I2S_BCK,
            .ws = I2S_WS,
            .dout = I2S_GPIO_UNUSED,
            .din = I2S_DI,
            .invert_flags = { .mclk_inv = false, .bclk_inv = false, .ws_inv = false },
        },
    };
    if (i2s_channel_init_std_mode(s_tx, &std) != ESP_OK) return -2;
    if (i2s_channel_init_tdm_mode(s_rx, &tdm) != ESP_OK) return -3;
    if (i2s_channel_enable(s_tx) != ESP_OK) return -4;
    if (i2s_channel_enable(s_rx) != ESP_OK) return -5;
    return 0;
}

static int codec_setup(void) {
    audio_codec_i2s_cfg_t i2s_cfg = { .port = I2S_NUM_0, .rx_handle = s_rx, .tx_handle = s_tx };
    const audio_codec_data_if_t *data_if = audio_codec_new_i2s_data(&i2s_cfg);
    if (!data_if) return -1;

    audio_codec_i2c_cfg_t i2c_cfg = { .port = I2C_PORT, .addr = ES8311_CODEC_DEFAULT_ADDR, .bus_handle = s_i2c };
    const audio_codec_ctrl_if_t *out_ctrl = audio_codec_new_i2c_ctrl(&i2c_cfg);
    const audio_codec_gpio_if_t *gpio_if = audio_codec_new_gpio();
    if (!out_ctrl || !gpio_if) return -2;
    es8311_codec_cfg_t es8311_cfg = {
        .ctrl_if = out_ctrl,
        .gpio_if = gpio_if,
        .codec_mode = ESP_CODEC_DEV_WORK_MODE_DAC,
        .pa_pin = PA_GPIO,
        .use_mclk = true,
        .hw_gain = { .pa_voltage = 5.0, .codec_dac_voltage = 3.3 },
    };
    const audio_codec_if_t *out_codec = es8311_codec_new(&es8311_cfg);
    if (!out_codec) return -3;
    esp_codec_dev_cfg_t dev_cfg = { .dev_type = ESP_CODEC_DEV_TYPE_OUT, .codec_if = out_codec, .data_if = data_if };
    s_out = esp_codec_dev_new(&dev_cfg);
    if (!s_out) return -4;

    i2c_cfg.addr = ES7210_CODEC_DEFAULT_ADDR;
    const audio_codec_ctrl_if_t *in_ctrl = audio_codec_new_i2c_ctrl(&i2c_cfg);
    if (!in_ctrl) return -5;
    es7210_codec_cfg_t es7210_cfg = {
        .ctrl_if = in_ctrl,
        .mic_selected = ES7210_SEL_MIC1 | ES7210_SEL_MIC2 | ES7210_SEL_MIC3 | ES7210_SEL_MIC4,
    };
    const audio_codec_if_t *in_codec = es7210_codec_new(&es7210_cfg);
    if (!in_codec) return -6;
    dev_cfg.dev_type = ESP_CODEC_DEV_TYPE_IN;
    dev_cfg.codec_if = in_codec;
    s_in = esp_codec_dev_new(&dev_cfg);
    if (!s_in) return -7;

    esp_codec_dev_sample_info_t in_fs = {
        .bits_per_sample = 16,
        .channel = TDM_SLOTS,
        .channel_mask = ESP_CODEC_DEV_MAKE_CHANNEL_MASK(0) | ESP_CODEC_DEV_MAKE_CHANNEL_MASK(1) |
                        ESP_CODEC_DEV_MAKE_CHANNEL_MASK(2) | ESP_CODEC_DEV_MAKE_CHANNEL_MASK(3),
        .sample_rate = SAMPLE_RATE,
    };
    if (esp_codec_dev_open(s_in, &in_fs) != ESP_CODEC_DEV_OK) return -8;
    // Gain masks use physical MIC numbering: MIC1 and MIC2, then MIC3 for the reference.
    esp_codec_dev_set_in_channel_gain(
        s_in, ESP_CODEC_DEV_MAKE_CHANNEL_MASK(0) | ESP_CODEC_DEV_MAKE_CHANNEL_MASK(1), MIC_GAIN_DB);
    esp_codec_dev_set_in_channel_gain(s_in, ESP_CODEC_DEV_MAKE_CHANNEL_MASK(2), REF_GAIN_DB);

    esp_codec_dev_sample_info_t out_fs = { .bits_per_sample = 16, .channel = 1, .sample_rate = SAMPLE_RATE };
    if (esp_codec_dev_open(s_out, &out_fs) != ESP_CODEC_DEV_OK) return -9;
    esp_codec_dev_set_out_vol(s_out, OUT_VOLUME);
    return 0;
}

// Reads the ES7210's four TDM slots into the per-mic tap and, while capturing, the mono mic stream.
static void capture_task(void *arg) {
    (void)arg;
    int16_t *frames = s_cap;
    int16_t *mono = s_mono;
    int64_t sq[TDM_SLOTS] = { 0 };
    int counted = 0;
    int reports = 0;
    int64_t level_sq = 0;
    int level_n = 0;
    for (;;) {
        if (esp_codec_dev_read(s_in, frames, CAP_BYTES) != ESP_CODEC_DEV_OK) {
            vTaskDelay(pdMS_TO_TICKS(5));
            continue;
        }
        for (int i = 0; i < CAP_FRAMES; i++) {
            int32_t a = frames[i * TDM_SLOTS + SLOT_MIC1];
            int32_t b = frames[i * TDM_SLOTS + SLOT_MIC2];
            level_sq += (int64_t)a * a + (int64_t)b * b;
        }
        level_n += CAP_FRAMES;
        if (level_n >= LEVEL_FRAMES) {
            float power = (float)level_sq / (2.0f * level_n) / (32768.0f * 32768.0f);
            level_frame_t f = { .db = 10.0f * log10f(power + 1e-10f), .speech = s_speech };
            xQueueSend(s_levels, &f, 0);
            level_sq = 0;
            level_n = 0;
        }
        taskENTER_CRITICAL(&s_tap_lock);
        for (int i = 0; i < CAP_FRAMES; i++) {
            s_tap1[s_tap_pos] = frames[i * TDM_SLOTS + SLOT_MIC1];
            s_tap2[s_tap_pos] = frames[i * TDM_SLOTS + SLOT_MIC2];
            s_tap_pos = (s_tap_pos + 1) & (TAP_LEN - 1);
        }
        taskEXIT_CRITICAL(&s_tap_lock);
        if (s_capture) {
            for (int i = 0; i < CAP_FRAMES; i++) {
                mono[i] = frames[i * TDM_SLOTS + SLOT_MIC1];
            }
            xStreamBufferSend(s_mic, mono, CAP_FRAMES * sizeof(int16_t), 0);
        }
        if (s_afe_data) {
            for (int i = 0; i < CAP_FRAMES; i++) {
                int16_t *f = &s_feed[s_feed_fill * AFE_CHANNELS];
                f[0] = frames[i * TDM_SLOTS + SLOT_MIC1];
                f[1] = frames[i * TDM_SLOTS + SLOT_MIC2];
                f[2] = frames[i * TDM_SLOTS + SLOT_REF];
                if (++s_feed_fill == s_feed_frames) {
                    s_afe->feed(s_afe_data, s_feed);
                    s_feed_fill = 0;
                }
            }
        }
        // Per-slot RMS every 2 s for the first minute after boot.
        if (reports < SLOT_REPORTS) {
            for (int i = 0; i < CAP_FRAMES * TDM_SLOTS; i++) {
                sq[i % TDM_SLOTS] += (int64_t)frames[i] * frames[i];
            }
            counted += CAP_FRAMES;
            if (counted >= SAMPLE_RATE * 2) {
                ESP_LOGI(TAG, "slot rms: s0=%d s1=%d s2=%d s3=%d",
                         (int)sqrtf((float)sq[0] / counted), (int)sqrtf((float)sq[1] / counted),
                         (int)sqrtf((float)sq[2] / counted), (int)sqrtf((float)sq[3] / counted));
                memset(sq, 0, sizeof sq);
                counted = 0;
                reports++;
            }
        }
    }
}

// Reads AFE results: wake word detections and the VAD state.
static void afe_task(void *arg) {
    esp_afe_sr_data_t *data = arg;
    int speech = 0;
    for (;;) {
        afe_fetch_result_t *res = s_afe->fetch(data);
        if (!res || res->ret_value == ESP_FAIL) continue;
        if (res->wakeup_state == WAKENET_DETECTED) {
            ESP_LOGI(TAG, "wake word (model %d, word %d)", res->wakenet_model_index, res->wake_word_index);
            s_wake = 1;
        }
        int now = res->vad_state == VAD_SPEECH;
        if (now != speech) {
            speech = now;
            s_speech = now;
        }
    }
}

// Drains the reply stream buffer into the speaker.
static void play_task(void *arg) {
    (void)arg;
    uint8_t *chunk = s_chunk;
    for (;;) {
        if (s_flush) {
            while (xStreamBufferReceive(s_play, chunk, PLAY_CHUNK, 0) > 0) {
            }
            s_flush = 0;
            s_eos = 0;
            s_busy = 0;
            continue;
        }
        size_t n = xStreamBufferReceive(s_play, chunk, PLAY_CHUNK, pdMS_TO_TICKS(20));
        if (n > 0) {
            esp_codec_dev_write(s_out, chunk, (int)n);
        } else if (s_eos) {
            s_eos = 0;
            s_busy = 0;
        }
    }
}

static int buffers_setup(void) {
    uint8_t *play = heap_caps_malloc(PLAY_BUF_BYTES + 1, MALLOC_CAP_SPIRAM | MALLOC_CAP_8BIT);
    uint8_t *mic = heap_caps_malloc(MIC_BUF_BYTES + 1, MALLOC_CAP_SPIRAM | MALLOC_CAP_8BIT);
    const uint32_t internal = MALLOC_CAP_INTERNAL | MALLOC_CAP_8BIT;
    s_tap1 = guarded_alloc("tap1", TAP_LEN * sizeof(int16_t), internal);
    s_tap2 = guarded_alloc("tap2", TAP_LEN * sizeof(int16_t), internal);
    s_cap = guarded_alloc("cap", CAP_BYTES, internal);
    s_mono = guarded_alloc("mono", CAP_FRAMES * sizeof(int16_t), internal);
    s_chunk = guarded_alloc("chunk", PLAY_CHUNK, internal);
    if (!play || !mic || !s_tap1 || !s_tap2 || !s_cap || !s_mono || !s_chunk) return -1;
    s_play = xStreamBufferCreateStatic(PLAY_BUF_BYTES, 1, play, &s_play_ctl);
    s_mic = xStreamBufferCreateStatic(MIC_BUF_BYTES, MIC_CHUNK, mic, &s_mic_ctl);
    s_levels = xQueueCreateWithCaps(LEVEL_QUEUE, sizeof(level_frame_t), MALLOC_CAP_SPIRAM | MALLOC_CAP_8BIT);
    return (s_play && s_mic && s_levels) ? 0 : -2;
}

void *audio_i2c_bus(void) {
    return s_i2c;
}

void audio_set_amp(int on) {
    gpio_set_level(PA_GPIO, on ? 1 : 0);
}

int audio_init(void) {
    s_tcm_fill[0] = 0;
    if (i2c_setup()) return -1;
    if (i2s_setup()) return -2;
    int rc = codec_setup();
    if (rc) {
        ESP_LOGE(TAG, "codec setup failed: %d", rc);
        return -3;
    }
    if (buffers_setup()) return -4;
    if (xTaskCreate(capture_task, "vc_mic", 4096, NULL, 7, NULL) != pdPASS) return -5;
    if (xTaskCreate(play_task, "vc_play", 3072, NULL, 5, NULL) != pdPASS) return -6;
    return 0;
}

int audio_sr_init(void) {
    size_t internal_before = heap_caps_get_free_size(MALLOC_CAP_INTERNAL);
    srmodel_list_t *models = esp_srmodel_init("model");
    if (!models || models->num == 0) {
        ESP_LOGW(TAG, "no speech models in the \"model\" partition");
        return -1;
    }
    afe_config_t *cfg = afe_config_init(AFE_FORMAT, models, AFE_TYPE_SR, AFE_MODE_HIGH_PERF);
    if (!cfg) return -2;
    cfg->memory_alloc_mode = AFE_MEMORY_ALLOC_MORE_PSRAM;
    cfg->afe_perferred_core = 1;
    cfg->afe_perferred_priority = 5;
    cfg->vad_min_noise_ms = VAD_END_MS;
    const esp_afe_sr_iface_t *afe = esp_afe_handle_from_config(cfg);
    esp_afe_sr_data_t *data = afe ? afe->create_from_config(cfg) : NULL;
    afe_config_free(cfg);
    if (!data) return -3;
    int frames = afe->get_feed_chunksize(data);
    int channels = afe->get_feed_channel_num(data);
    if (channels != AFE_CHANNELS) {
        ESP_LOGE(TAG, "AFE wants %d channels, not %d", channels, AFE_CHANNELS);
        afe->destroy(data);
        return -4;
    }
    s_feed = guarded_alloc("feed", (size_t)frames * AFE_CHANNELS * sizeof(int16_t), MALLOC_CAP_SPIRAM | MALLOC_CAP_8BIT);
    if (!s_feed) return -5;
    s_afe = afe;
    s_feed_frames = frames;
    if (xTaskCreatePinnedToCoreWithCaps(afe_task, "vc_afe", 8192, data, 5, NULL, 1,
                                        MALLOC_CAP_SPIRAM | MALLOC_CAP_8BIT) != pdPASS) {
        return -6;
    }
    s_afe_data = data;
    ESP_LOGI(TAG, "afe ready: %u B internal ram used, %u B free, %u B largest block",
             (unsigned)(internal_before - heap_caps_get_free_size(MALLOC_CAP_INTERNAL)),
             (unsigned)heap_caps_get_free_size(MALLOC_CAP_INTERNAL),
             (unsigned)heap_caps_get_largest_free_block(MALLOC_CAP_INTERNAL));
    return 0;
}

int audio_wake_take(void) {
    int w = s_wake;
    s_wake = 0;
    return w;
}

int audio_level_frame(float *db, int *speech) {
    level_frame_t f;
    if (!s_levels || xQueueReceive(s_levels, &f, 0) != pdTRUE) return 0;
    *db = f.db;
    *speech = f.speech;
    return 1;
}

void audio_set_volume(int level) {
    if (s_out) esp_codec_dev_set_out_vol(s_out, level);
}

int audio_write(const void *buf, size_t len) {
    return esp_codec_dev_write(s_out, (void *)buf, (int)len) == ESP_CODEC_DEV_OK ? (int)len : 0;
}

int audio_read(void *buf, size_t len) {
    if (!s_mic) return 0;
    return (int)xStreamBufferReceive(s_mic, buf, len, pdMS_TO_TICKS(1000));
}

void audio_capture(int on) {
    if (on && s_mic) {
        uint8_t scratch[256];
        while (xStreamBufferReceive(s_mic, scratch, sizeof scratch, 0) > 0) {
        }
    }
    s_capture = on;
}

void audio_tap_latest(int16_t *mic1, int16_t *mic2, size_t n) {
    if (!s_tap1 || !s_tap2) return;
    if (n > TAP_LEN) n = TAP_LEN;
    taskENTER_CRITICAL(&s_tap_lock);
    uint32_t start = (s_tap_pos - (uint32_t)n) & (TAP_LEN - 1);
    for (size_t i = 0; i < n; i++) {
        uint32_t k = (start + (uint32_t)i) & (TAP_LEN - 1);
        mic1[i] = s_tap1[k];
        mic2[i] = s_tap2[k];
    }
    taskEXIT_CRITICAL(&s_tap_lock);
}

void audio_play_begin(void) {
    s_eos = 0;
    s_busy = 1;
    s_accept = 1;
}

int audio_play_push(const void *buf, size_t len) {
    if (!s_accept || !s_play) return 0;
    return (int)xStreamBufferSend(s_play, buf, len, pdMS_TO_TICKS(2000));
}

void audio_play_end(void) {
    s_accept = 0;
    s_eos = 1;
}

void audio_play_stop(void) {
    s_accept = 0;
    s_flush = 1;
}

int audio_play_active(void) {
    return s_busy;
}
