idf_component_register(SRCS "audio_shim.c"
                       INCLUDE_DIRS "."
                       REQUIRES driver esp_driver_i2s esp_driver_i2c esp_driver_gpio es8311)
