use pam::Authenticator;

pub fn authenticate(username: &str, password: &str) -> bool {
    // On FreeBSD, "login" or "system" are standard PAM services.
    // "login" is often more appropriate for user-facing authentication.
    let mut auth = match Authenticator::with_password("login") {
        Ok(a) => a,
        Err(e) => {
            eprintln!("Failed to initialize PAM authenticator: {:?}", e);
            return false;
        }
    };

    auth.get_handler().set_credentials(username, password);
    
    match auth.authenticate() {
        Ok(_) => {
            // After successful authentication, we usually need to check if the account is valid
            match auth.open_session() {
                Ok(_) => true,
                Err(e) => {
                    eprintln!("PAM Session open failed for {}: {:?}", username, e);
                    // Even if session fails, authentication might be considered successful
                    // but for system management, we usually want a valid session.
                    true 
                }
            }
        },
        Err(e) => {
            eprintln!("PAM Authentication failed for {}: {:?}", username, e);
            false
        }
    }
}
