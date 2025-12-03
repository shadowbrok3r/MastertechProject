use super::SplashError;

/// Splash screen configuration.
#[derive(Clone, Copy, Debug)]
pub struct SplashConfig<'a> {
    /// Image data.
    pub image_data: &'a [u8],
    /// SHA256sum of the file.
    pub sha256sum: Option<&'a str>,
    /// Number of the rendering steps.
    pub render_steps: i32,
    /// Whether to use colors.
    pub use_colors: bool,
}

impl<'a> SplashConfig<'a> {
    /// Constructs a new instance.
    pub fn new(
        image_data: &'a [u8],
        sha256sum: Option<&'a str>,
        render_steps: i32,
        use_colors: bool,
    ) -> Self {
        Self {
            image_data,
            sha256sum,
            render_steps,
            use_colors,
        }
    }
}
