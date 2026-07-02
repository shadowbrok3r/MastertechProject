use dioxus::prelude::*;
use database::Database;

#[derive(Props, Clone, PartialEq)]
pub struct LoginPageProps {
    pub on_login: EventHandler<(bool, Option<String>)>,
}

#[component]
pub fn LoginPage(props: LoginPageProps) -> Element {
    let mut username = use_signal(|| String::new());
    let mut password = use_signal(|| String::new());
    let mut busy = use_signal(|| false);
    let mut error = use_signal(|| Option::<String>::None);

    let mut submit = move || {
        let user = username();
        let pass = password();
        if user.is_empty() || pass.is_empty() {
            error.set(Some("Enter username and password".into()));
            return;
        }
        error.set(None);
        busy.set(true);
        spawn({
            let user = user.clone();
            let pass = pass.clone();
            async move {
                let email = if user.ends_with("@pclaptops.com") { user } else { format!("{user}@pclaptops.com") };
                let res = Database::new(email.clone(), pass.clone(), None).await;
                match res {
                    Ok(db) if db.jwt.is_some() && db.user.is_some() => {
                        if let Some(jwt) = &db.jwt {
                            crate::save_session(&crate::SavedSession {
                                token: Some(jwt.clone()),
                                email: Some(email.clone()),
                            });
                        }
                        props.on_login.call((true, None));
                    }
                    Ok(_) => props.on_login.call((false, Some("No session returned".into()))),
                    Err(e) => props.on_login.call((false, Some(e.to_string()))),
                }
                busy.set(false);
            }
        });
    };

    rsx! {
        div { class: "min-h-screen bg-galaxy flex items-center justify-center p-6",
            div { class: "card-cosmic w-full max-w-xs p-5 space-y-4",
                // Logo
                div { class: "flex items-center gap-2 justify-center mb-2",
                    div { class: "w-8 h-8 rounded-lg grad-crimson" }
                    span { class: "text-base font-bold text-star-white", "Mastertech" }
                }
                input {
                    class: "w-full text-sm",
                    r#type: "text",
                    placeholder: "Username",
                    value: username(),
                    oninput: move |e| username.set(e.value()),
                }
                input {
                    class: "w-full text-sm",
                    r#type: "password",
                    placeholder: "Password",
                    value: password(),
                    oninput: move |e| password.set(e.value()),
                }
                if let Some(e) = error() {
                    div { class: "text-xs text-warning-red", {e} }
                }
                button {
                    class: "btn-nebula w-full",
                    disabled: busy(),
                    onclick: move |_| submit(),
                    if busy() { "Signing in..." } else { "Sign In" }
                }
            }
        }
    }
}
