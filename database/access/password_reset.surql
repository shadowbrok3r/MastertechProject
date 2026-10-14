-- Redeems a password_reset code: params $email, $code, $new_password.
-- A wrong code increments `attempts`; five misses, expiry or a newer code invalidates it.
DEFINE ACCESS OVERWRITE password_reset ON DATABASE TYPE RECORD
    SIGNIN {
        IF string::len(<string> ($new_password ?? '')) < 8 {
            THROW 'The new password must be at least 8 characters.';
        };
        LET $target = (SELECT id, active FROM user WHERE email = string::lowercase(string::trim(<string> ($email ?? ''))) LIMIT 1)[0];
        LET $grant = IF $target != NONE AND $target.active == true {
            (SELECT id, code_hash, attempts, expires_at, used_at, created_at FROM password_reset WHERE user = $target.id ORDER BY created_at DESC LIMIT 1)[0]
        } ELSE {
            NONE
        };
        LET $live = $grant != NONE AND $grant.used_at == NONE AND $grant.expires_at > time::now() AND $grant.attempts < 5;
        LET $match = IF $live {
            crypto::argon2::compare($grant.code_hash, string::trim(<string> ($code ?? '')))
        } ELSE {
            false
        };
        IF $live AND !$match {
            UPDATE $grant.id SET attempts += 1;
        };
        IF $match {
            UPDATE $grant.id SET used_at = time::now();
            UPDATE $target.id SET password = crypto::argon2::generate(<string> $new_password);
            CREATE audit_log CONTENT { password_reset: { user: $target.id, code: $grant.id }, time: time::now() };
        };
        RETURN IF $match { $target.id } ELSE { NONE };
    }
    DURATION FOR TOKEN 1m, FOR SESSION 1m;
