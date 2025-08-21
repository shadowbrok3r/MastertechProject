/** @type {import('tailwindcss').Config} */
module.exports = {
  mode: "all",
  content: ["./src/**/*.{rs,html,css}", "./dist/**/*.html"],
  theme: {
    extend: {
      colors: {
        panel: "#0c0c0f", // visuals.panel_fill [12,12,15]
        window: "#0b0b0f", // visuals.window_fill [11,11,15]
        stroke: "#4d5e8a", // visuals.window_stroke
        warn: "#3db99d",   // warn_fg_color
        error: "#ff3766",  // error_fg_color
        link: "#875581",   // hyperlink_color
        faint: "#111216",  // faint_bg_color
        extreme: "#090c15", // extreme_bg_color
        card: "#0f1014",
        accent1: "#6a659b", // hovered stroke
        accent2: "#0bf4c0", // slider_trailing_fill / green used
        accent3: "#d9ff00", // yellow for due soon
      }
    },
  },
  plugins: [],
};
