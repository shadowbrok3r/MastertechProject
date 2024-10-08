use log::info;
use ring::aead;
use ring::rand::{SecureRandom, SystemRandom};
use ring::pbkdf2;
use std::num::NonZeroU32;
use std::fs::{read, File};
use std::io::Write;
use crate::pages::login_page::Login;

const KEY_LEN: usize = 32;
const SALT_LEN: usize = 16;
const ITERATIONS: Option<NonZeroU32> = NonZeroU32::new(100_000);

fn generate_key(password: &[u8], salt: &[u8]) -> [u8; KEY_LEN] {
    let mut key = [0u8; KEY_LEN];
    pbkdf2::derive(
        pbkdf2::PBKDF2_HMAC_SHA256,
        ITERATIONS.unwrap(),
        salt,
        password,
        &mut key,
    );
    key
}

fn encrypt_data(data: &[u8], key: &[u8]) -> Vec<u8> {
    let sealing_key = aead::LessSafeKey::new(
        aead::UnboundKey::new(&aead::AES_256_GCM, key)
            .expect("Failed to create unbound key")
        );
    let nonce = aead::Nonce::assume_unique_for_key([0; 12]);
    let mut in_out = data.to_vec();
    let tag = sealing_key.seal_in_place_separate_tag(nonce, aead::Aad::empty(), &mut in_out).unwrap();
    in_out.extend(tag.as_ref());
    in_out
}

fn decrypt_data(data: &[u8], key: &[u8]) -> Vec<u8> {
    let opening_key = aead::LessSafeKey::new(
        aead::UnboundKey::new(&aead::AES_256_GCM, key)
        .expect("Failed to create unbound key")
    );
    let nonce = aead::Nonce::assume_unique_for_key([0; 12]);
    let mut in_out = data.to_vec();
    opening_key.open_in_place(nonce, aead::Aad::empty(), &mut in_out).expect("Decryption failed");
    in_out.truncate(in_out.len() - aead::AES_256_GCM.tag_len());
    in_out
}

pub fn save_encrypted_user_data(user_data: &Login, password: &[u8]) 
    -> anyhow::Result<(), anyhow::Error> 
{
    info!("User data: {user_data:?}");
    let salt = generate_salt();
    let key = generate_key(password, &salt);
    let serialized_data = bincode::serialize(user_data)?;
    // let serialized_data = serde_json::to_vec(&user_data)?;
    let encrypted_data = encrypt_data(&serialized_data, &key);
    
    let mut file = File::create("data.enc")?;
    file.write_all(&salt)?;
    file.write_all(&encrypted_data)?;
    Ok(())
}

fn generate_salt() -> [u8; SALT_LEN] {
    let rng = SystemRandom::new();
    let mut salt = [0u8; SALT_LEN];
    rng.fill(&mut salt).unwrap();
    salt
}




pub fn load_encrypted_user_data(password: &[u8]) -> Option<Login> {
    match read("data.enc"){
        Ok(data) => {
            let salt = &data[..SALT_LEN];
            let encrypted_data = &data[SALT_LEN..];
            let key = generate_key(password, salt);
            let decrypted_data = decrypt_data(encrypted_data, &key);
            // let login: Login = serde_json::from_slice(&decrypted_data).unwrap();
            let login: Login = bincode::deserialize(&decrypted_data).unwrap();
            Some(login)
        },
        Err(e) => {
            info!("*.enc File not found {e:?}");
            None
        },
    }
}

