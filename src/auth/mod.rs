use crate::models::user::User;

#[derive(Debug)]
pub enum AuthTypes {
    UnAuthorized,
    AuthorizedNotSignedUp(String),
    AuthorizedUser(User),
}

impl AuthTypes {
    pub fn as_authorized_user(&self) -> Option<&User> {
        if let Self::AuthorizedUser(v) = self {
            Some(v)
        } else {
            None
        }
    }
}
