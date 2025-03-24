use super::ColorTheme;

impl ColorTheme {
    /// Author : OwOSwordsman <owoswordsman@gmail.com>
    /// An unofficial GitHub theme, generated using colors from: <https://primer.style/primitives/colors>
    pub const GITHUB_DARK: ColorTheme = ColorTheme {
        name: "Github Dark",
        dark: true,
        bg: "#0d1117",          // default
        cursor: "#d29922",      // attention.fg
        selection: "#0c2d6b",   // scale.blue.8
        comments: "#8b949e",    // fg.muted
        functions: "#d2a8ff",   // scale.purple.2
        keywords: "#ff7b72",    // scale.red.3
        literals: "#c9d1d9",    // fg.default
        numerics: "#79c0ff",    // scale.blue.2
        punctuation: "#c9d1d9", // fg.default
        strs: "#a5d6ff",        // scale.blue.1
        types: "#ffa657",       // scale.orange.2
        special: "#a5d6ff",     // scale.blue.1
        variable: "#a5d6ff",    // scale.blue.1
        symbol: "#03fcba",      // Bright teal for symbols
        embedded: "#03fcba",    // Match embedded content to symbol
        afterdollarinstring: "#03fcba",  // $ symbol matches symbol color
        embeddedvariable: "#a5d6ff",     // Matches variable color
        subexpression: "#03fcba",        // Matches embedded content
    };
}