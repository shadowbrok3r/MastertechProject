use super::ColorTheme;

impl ColorTheme {
    pub const TOKYO_DARK: ColorTheme = ColorTheme {
        name: "Tokyo Dark",
        dark: true,
        bg: "#1a1b2b",        // Deep cosmic navy, slightly bluer than before
        cursor: "#b0e0ff",     // Bright icy blue, glowing like a star
        selection: "#ff00ff",  // Vibrant magenta for a bold highlight
        comments: "#5e5f8a",   // Muted purple-gray, like distant nebulae
        functions: "#ff6bd6",  // Hot pink, vibrant and eye-catching
        keywords: "#ff8f56",   // Bright orange, warm and energetic
        literals: "#c0c5ff",   // Soft lavender-blue, glowing subtly
        numerics: "#00d4ff",   // Electric cyan, crisp and spacey
        punctuation: "#d8b0ff", // Light purple, delicate yet visible
        strs: "#ffcc66",       // Warm orange-yellow, like a glowing star
        types: "#ff9966",      // Peachy orange, distinct yet harmonious
        special: "#66ccff",    // Sky blue, bright and celestial
        variable: "#ff6699",   // Deep pink, bold and lively
        symbol: "#33ffcc",     // Turquoise, radiant and futuristic
        embedded: "#33ffcc",   // Match embedded content to symbol
        afterdollarinstring: "#33ffcc",  // $ symbol matches symbol color
        embeddedvariable: "#ff6699",     // Matches variable color
        subexpression: "#33ffcc",        // Matches embedded content
    };
}
