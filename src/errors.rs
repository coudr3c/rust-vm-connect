#[derive(Debug)]
pub struct SSMError {
    pub kind: SSMErrorKind,
    pub msg: String
}

#[derive(Debug)]
pub enum SSMErrorKind {
    StartSessionError,
    CommandSpawnError
}