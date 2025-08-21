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
        if user.is_empty() || pass.is_empty() { error.set(Some("Please enter username and password".into())); return; }
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
                        // persist session
                        if let Some(jwt) = &db.jwt { 
                            crate::save_session(&crate::SavedSession { token: Some(jwt.as_insecure_token().to_string()), email: Some(email.clone()) });
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
        div { class: "min-h-screen bg-[#0b0b0f] text-slate-200 flex items-center justify-center",
            div { class: "w-full max-w-sm bg-[#0c0c10]/60 backdrop-blur rounded-lg border border-[#2a2c5d]/60 p-6 shadow",
                h1 { class: "text-lg font-semibold mb-4", "Mastertech Mobile" }
                div { class: "space-y-3",
                    input { class: "w-full bg-[#111216] rounded px-3 py-2 text-sm border border-[#2a2c5d]/60",
                        placeholder: "username",
                        value: username(),
                        oninput: move |e| username.set(e.value())
                    }
                    input { class: "w-full bg-[#111216] rounded px-3 py-2 text-sm border border-[#2a2c5d]/60",
                        r#type: "password",
                        placeholder: "password",
                        value: password(),
                        oninput: move |e| password.set(e.value())
                    }
                    if let Some(e) = error() { div { class: "text-sm text-red-400", {e} } }
                    button { class: "w-full px-3 py-2 rounded border border-[#2a2c5d]/60 hover:bg-[#251d3d]/50",
                        disabled: busy(),
                        onclick: move |_| submit(),
                        if busy() { span { "Signing in..." } } else { span { "Sign in" } }
                    }
                }
            }
        }
    }
}
