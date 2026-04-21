use keyring::Entry;

use crate::error::AppResult;

const SERVICE: &str = "com.simba.auditapp";

pub fn set(account: &str, secret: &str) -> AppResult<()> {
    let entry = Entry::new(SERVICE, account)?;
    entry.set_password(secret)?;
    Ok(())
}

pub fn get(account: &str) -> AppResult<String> {
    let entry = Entry::new(SERVICE, account)?;
    Ok(entry.get_password()?)
}

pub fn remove(account: &str) -> AppResult<()> {
    let entry = Entry::new(SERVICE, account)?;
    entry.delete_credential()?;
    Ok(())
}
