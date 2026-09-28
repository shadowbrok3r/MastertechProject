idf_component_register(SRCS "display_shim.c"
                       INCLUDE_DIRS "."
                       REQUIRES driver esp_lcd esp_lcd_st7703 esp_lcd_touch esp_lcd_touch_gt911 esp_driver_gpio esp_hw_support esp_lvgl_port lvgl)
