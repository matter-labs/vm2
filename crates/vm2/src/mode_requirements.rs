/// Requirements for the VM execution mode that can be placed by instructions.
#[derive(Debug)]
pub struct ModeRequirements(pub(crate) u8);

impl ModeRequirements {
    /// Creates new requirements.
    pub const fn new(kernel_only: bool, cannot_use_in_static: bool) -> Self {
        Self((kernel_only as u8) | ((cannot_use_in_static as u8) << 1))
    }

    /// Creates default requirements that always hold.
    pub const fn none() -> Self {
        Self::new(false, false)
    }

    pub(crate) fn met(&self, is_kernel: bool, is_static: bool) -> bool {
        let enabled_modes = (is_kernel as u8) | ((!is_static as u8) << 1);
        enabled_modes & self.0 == self.0
    }
}
