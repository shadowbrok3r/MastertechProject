use super::ColorTheme;

impl ColorTheme {
    pub const TOKYO_DARK: ColorTheme = ColorTheme {
        name: "Tokyo Dark",
        dark: true,
        bg: "#1a1b26",
        cursor: "#c0caf5",
        selection: "#4903fc", // "#33467c",
        comments: "#565f89",
        functions: "#bb9af7",
        keywords: "#f7768e",
        literals: "#a9b1d6",
        numerics: "#7aa2f7",
        punctuation: "#a9b1d6",
        strs: "#9ece6a",
        types: "#e0af68",
        special: "#7dcfff",
        variable: "#ff9e64", // Orange for variables
        symbol: "#03fcba",
        embedded: "#03fcba",  // Light Green for symbols ($)
    };
}
