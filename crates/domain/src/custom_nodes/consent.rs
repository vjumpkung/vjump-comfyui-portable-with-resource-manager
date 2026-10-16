use super::NodeError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallConsent {
    pub plan_id: String,
    pub binding: String,
    pub execute_node_code: bool,
    pub run_install_py: bool,
    pub allow_source_builds: bool,
    pub acknowledge_warnings: bool,
    pub remember_source: bool,
    pub remember_install_py: bool,
}
impl InstallConsent {
    pub fn check(
        &self,
        plan_id: &str,
        binding: &str,
        warnings: bool,
        has_hook: bool,
    ) -> Result<(), NodeError> {
        if self.plan_id != plan_id
            || self.binding != binding
            || !self.execute_node_code
            || (warnings && !self.acknowledge_warnings)
            || (self.run_install_py && !has_hook)
            || (self.remember_install_py && (!self.remember_source || !self.run_install_py))
        {
            Err(NodeError::ConsentMismatch)
        } else {
            Ok(())
        }
    }
}
