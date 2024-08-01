# Details

Date : 2024-07-13 17:34:50

Directory /home/shadowbroker/Documents/Tech/Programming/MtechServer2.0

Total : 100 files,  20661 codes, 1826 comments, 2885 blanks, all 25372 lines

[Summary](results.md) / Details / [Diff Summary](diff.md) / [Diff Details](diff-details.md)

## Files
| filename | language | code | comment | blank | total |
| :--- | :--- | ---: | ---: | ---: | ---: |
| [.cargo/config.toml](/.cargo/config.toml) | TOML | 4 | 9 | 7 | 20 |
| [.github/workflows/pages.yml](/.github/workflows/pages.yml) | YAML | 31 | 39 | 5 | 75 |
| [.github/workflows/rust.yml](/.github/workflows/rust.yml) | YAML | 20 | 31 | 9 | 60 |
| [Cargo.lock](/Cargo.lock) | TOML | 6,687 | 2 | 691 | 7,380 |
| [Cargo.toml](/Cargo.toml) | TOML | 13 | 0 | 2 | 15 |
| [Dockerfile](/Dockerfile) | Docker | 14 | 28 | 12 | 54 |
| [Trunk.toml](/Trunk.toml) | TOML | 9 | 60 | 10 | 79 |
| [database/.env](/database/.env) | Properties | 1 | 0 | 0 | 1 |
| [database/Cargo.toml](/database/Cargo.toml) | TOML | 13 | 4 | 2 | 19 |
| [database/backups/06.27.2024.surql](/database/backups/06.27.2024.surql) | SurrealQL | 383 | 63 | 62 | 508 |
| [database/backups/Local/Backup.surql](/database/backups/Local/Backup.surql) | SurrealQL | 340 | 60 | 59 | 459 |
| [database/backups/Local/database.surql](/database/backups/Local/database.surql) | SurrealQL | 332 | 57 | 60 | 449 |
| [database/backups/Local/localhost-2024-06-17.surql](/database/backups/Local/localhost-2024-06-17.surql) | SurrealQL | 339 | 60 | 61 | 460 |
| [database/backups/Local/v2-data-2024-06-17.surql](/database/backups/Local/v2-data-2024-06-17.surql) | SurrealQL | 326 | 58 | 50 | 434 |
| [database/backups/Local/v2-schema-2024-06-17.surql](/database/backups/Local/v2-schema-2024-06-17.surql) | SurrealQL | 82 | 27 | 28 | 137 |
| [database/backups/Production/master-techapp-2024-07-04-TEST.surql](/database/backups/Production/master-techapp-2024-07-04-TEST.surql) | SurrealQL | 375 | 62 | 65 | 502 |
| [database/backups/Production/master-techapp-2024-07-04.surql](/database/backups/Production/master-techapp-2024-07-04.surql) | SurrealQL | 372 | 60 | 64 | 496 |
| [database/backups/db.surql](/database/backups/db.surql) | SurrealQL | 358 | 63 | 62 | 483 |
| [database/src/lib.rs](/database/src/lib.rs) | Rust | 130 | 7 | 29 | 166 |
| [database/src/schema/mod.rs](/database/src/schema/mod.rs) | Rust | 524 | 71 | 69 | 664 |
| [docker-compose.yml](/docker-compose.yml) | YAML | 97 | 37 | 5 | 139 |
| [frontend/.env](/frontend/.env) | Properties | 1 | 0 | 0 | 1 |
| [frontend/Cargo.toml](/frontend/Cargo.toml) | TOML | 49 | 11 | 8 | 68 |
| [frontend/assets/manifest.json](/frontend/assets/manifest.json) | JSON | 23 | 0 | 1 | 24 |
| [frontend/assets/sw.js](/frontend/assets/sw.js) | JavaScript | 21 | 2 | 3 | 26 |
| [frontend/index.html](/frontend/index.html) | HTML | 106 | 10 | 24 | 140 |
| [frontend/src/app_state.rs](/frontend/src/app_state.rs) | Rust | 431 | 38 | 55 | 524 |
| [frontend/src/dummy_worker.rs](/frontend/src/dummy_worker.rs) | Rust | 5 | 0 | 0 | 5 |
| [frontend/src/lib.rs](/frontend/src/lib.rs) | Rust | 1 | 0 | 0 | 1 |
| [frontend/src/main.rs](/frontend/src/main.rs) | Rust | 334 | 39 | 36 | 409 |
| [frontend/src/pages/downloads_page.rs](/frontend/src/pages/downloads_page.rs) | Rust | 59 | 3 | 12 | 74 |
| [frontend/src/pages/login_page.rs](/frontend/src/pages/login_page.rs) | Rust | 173 | 4 | 22 | 199 |
| [frontend/src/pages/main_page.rs](/frontend/src/pages/main_page.rs) | Rust | 28 | 1 | 2 | 31 |
| [frontend/src/pages/menu_bar.rs](/frontend/src/pages/menu_bar.rs) | Rust | 114 | 9 | 11 | 134 |
| [frontend/src/pages/mod.rs](/frontend/src/pages/mod.rs) | Rust | 6 | 0 | 0 | 6 |
| [frontend/src/pages/signup_page.rs](/frontend/src/pages/signup_page.rs) | Rust | 158 | 7 | 30 | 195 |
| [frontend/src/pages/webconsole_page.rs](/frontend/src/pages/webconsole_page.rs) | Rust | 0 | 0 | 1 | 1 |
| [frontend/src/tabs/aging_tasks/mod.rs](/frontend/src/tabs/aging_tasks/mod.rs) | Rust | 6 | 30 | 4 | 40 |
| [frontend/src/tabs/ai_playground/mod.rs](/frontend/src/tabs/ai_playground/mod.rs) | Rust | 184 | 3 | 29 | 216 |
| [frontend/src/tabs/completed_tasks/mod.rs](/frontend/src/tabs/completed_tasks/mod.rs) | Rust | 18 | 6 | 2 | 26 |
| [frontend/src/tabs/github_issue/mod.rs](/frontend/src/tabs/github_issue/mod.rs) | Rust | 87 | 1 | 21 | 109 |
| [frontend/src/tabs/mod.rs](/frontend/src/tabs/mod.rs) | Rust | 47 | 0 | 3 | 50 |
| [frontend/src/tabs/my_tasks/mod.rs](/frontend/src/tabs/my_tasks/mod.rs) | Rust | 28 | 24 | 4 | 56 |
| [frontend/src/tabs/quote_fulfilled_tasks/mod.rs](/frontend/src/tabs/quote_fulfilled_tasks/mod.rs) | Rust | 0 | 0 | 1 | 1 |
| [frontend/src/tabs/store_tasks/mod.rs](/frontend/src/tabs/store_tasks/mod.rs) | Rust | 22 | 2 | 4 | 28 |
| [frontend/src/tabs/terminal/chart.rs](/frontend/src/tabs/terminal/chart.rs) | Rust | 104 | 0 | 13 | 117 |
| [frontend/src/tabs/terminal/mod.rs](/frontend/src/tabs/terminal/mod.rs) | Rust | 35 | 1 | 7 | 43 |
| [frontend/src/tabs/toolbox/mod.rs](/frontend/src/tabs/toolbox/mod.rs) | Rust | 38 | 0 | 12 | 50 |
| [frontend/src/tabs/toolbox/storage_api.rs](/frontend/src/tabs/toolbox/storage_api.rs) | Rust | 302 | 8 | 49 | 359 |
| [frontend/src/tabs/web_console/charts.rs](/frontend/src/tabs/web_console/charts.rs) | Rust | 84 | 10 | 14 | 108 |
| [frontend/src/tabs/web_console/display.rs](/frontend/src/tabs/web_console/display.rs) | Rust | 167 | 2 | 19 | 188 |
| [frontend/src/tabs/web_console/mod.rs](/frontend/src/tabs/web_console/mod.rs) | Rust | 47 | 1 | 12 | 60 |
| [frontend/src/tabs/web_console/websockets.rs](/frontend/src/tabs/web_console/websockets.rs) | Rust | 435 | 30 | 59 | 524 |
| [frontend/src/utilities/ai/chat.rs](/frontend/src/utilities/ai/chat.rs) | Rust | 70 | 0 | 8 | 78 |
| [frontend/src/utilities/ai/conv.rs](/frontend/src/utilities/ai/conv.rs) | Rust | 70 | 14 | 16 | 100 |
| [frontend/src/utilities/ai/gpts.rs](/frontend/src/utilities/ai/gpts.rs) | Rust | 4 | 7 | 7 | 18 |
| [frontend/src/utilities/ai/mod.rs](/frontend/src/utilities/ai/mod.rs) | Rust | 9 | 2 | 6 | 17 |
| [frontend/src/utilities/ai/model.rs](/frontend/src/utilities/ai/model.rs) | Rust | 7 | 0 | 2 | 9 |
| [frontend/src/utilities/ai/oa_client.rs](/frontend/src/utilities/ai/oa_client.rs) | Rust | 9 | 0 | 4 | 13 |
| [frontend/src/utilities/ai/tool_call.rs](/frontend/src/utilities/ai/tool_call.rs) | Rust | 157 | 22 | 29 | 208 |
| [frontend/src/utilities/ai/tools/ai_tools.rs](/frontend/src/utilities/ai/tools/ai_tools.rs) | Rust | 24 | 0 | 5 | 29 |
| [frontend/src/utilities/ai/tools/mod.rs](/frontend/src/utilities/ai/tools/mod.rs) | Rust | 16 | 3 | 8 | 27 |
| [frontend/src/utilities/ai/tools/spec.rs](/frontend/src/utilities/ai/tools/spec.rs) | Rust | 50 | 4 | 12 | 66 |
| [frontend/src/utilities/ai/tools/weather.rs](/frontend/src/utilities/ai/tools/weather.rs) | Rust | 40 | 5 | 8 | 53 |
| [frontend/src/utilities/ai/utils/mod.rs](/frontend/src/utilities/ai/utils/mod.rs) | Rust | 2 | 3 | 4 | 9 |
| [frontend/src/utilities/ai/utils/x_value.rs](/frontend/src/utilities/ai/utils/x_value.rs) | Rust | 16 | 0 | 4 | 20 |
| [frontend/src/utilities/displays/chats/highlighter.rs](/frontend/src/utilities/displays/chats/highlighter.rs) | Rust | 164 | 10 | 19 | 193 |
| [frontend/src/utilities/displays/chats/markdown_editor.rs](/frontend/src/utilities/displays/chats/markdown_editor.rs) | Rust | 256 | 28 | 49 | 333 |
| [frontend/src/utilities/displays/chats/mod.rs](/frontend/src/utilities/displays/chats/mod.rs) | Rust | 237 | 6 | 40 | 283 |
| [frontend/src/utilities/displays/chats/parser.rs](/frontend/src/utilities/displays/chats/parser.rs) | Rust | 228 | 59 | 44 | 331 |
| [frontend/src/utilities/displays/chats/viewer.rs](/frontend/src/utilities/displays/chats/viewer.rs) | Rust | 158 | 2 | 14 | 174 |
| [frontend/src/utilities/displays/mod.rs](/frontend/src/utilities/displays/mod.rs) | Rust | 3 | 0 | 1 | 4 |
| [frontend/src/utilities/displays/modals/ai_chat.rs](/frontend/src/utilities/displays/modals/ai_chat.rs) | Rust | 0 | 0 | 1 | 1 |
| [frontend/src/utilities/displays/modals/create_task_modal.rs](/frontend/src/utilities/displays/modals/create_task_modal.rs) | Rust | 293 | 21 | 38 | 352 |
| [frontend/src/utilities/displays/modals/mod.rs](/frontend/src/utilities/displays/modals/mod.rs) | Rust | 280 | 23 | 44 | 347 |
| [frontend/src/utilities/displays/modals/task_modal.rs](/frontend/src/utilities/displays/modals/task_modal.rs) | Rust | 734 | 46 | 83 | 863 |
| [frontend/src/utilities/displays/tasks/mod.rs](/frontend/src/utilities/displays/tasks/mod.rs) | Rust | 3 | 0 | 0 | 3 |
| [frontend/src/utilities/displays/tasks/sub_menu.rs](/frontend/src/utilities/displays/tasks/sub_menu.rs) | Rust | 0 | 15 | 3 | 18 |
| [frontend/src/utilities/displays/tasks/task_cards.rs](/frontend/src/utilities/displays/tasks/task_cards.rs) | Rust | 158 | 31 | 16 | 205 |
| [frontend/src/utilities/displays/tasks/task_layout.rs](/frontend/src/utilities/displays/tasks/task_layout.rs) | Rust | 280 | 4 | 33 | 317 |
| [frontend/src/utilities/filter.rs](/frontend/src/utilities/filter.rs) | Rust | 64 | 8 | 12 | 84 |
| [frontend/src/utilities/get_other.rs](/frontend/src/utilities/get_other.rs) | Rust | 52 | 0 | 11 | 63 |
| [frontend/src/utilities/get_tasks.rs](/frontend/src/utilities/get_tasks.rs) | Rust | 79 | 1 | 12 | 92 |
| [frontend/src/utilities/handle_live_data.rs](/frontend/src/utilities/handle_live_data.rs) | Rust | 197 | 62 | 32 | 291 |
| [frontend/src/utilities/interact_tasks.rs](/frontend/src/utilities/interact_tasks.rs) | Rust | 155 | 3 | 22 | 180 |
| [frontend/src/utilities/mod.rs](/frontend/src/utilities/mod.rs) | Rust | 230 | 27 | 36 | 293 |
| [frontend/src/utilities/sortable.rs](/frontend/src/utilities/sortable.rs) | Rust | 34 | 13 | 7 | 54 |
| [frontend/src/utilities/task_crud.rs](/frontend/src/utilities/task_crud.rs) | Rust | 78 | 19 | 13 | 110 |
| [frontend/src/utilities/ui_tools/autocomplete.rs](/frontend/src/utilities/ui_tools/autocomplete.rs) | Rust | 208 | 65 | 25 | 298 |
| [frontend/src/utilities/ui_tools/carl_dark.rs](/frontend/src/utilities/ui_tools/carl_dark.rs) | Rust | 330 | 92 | 61 | 483 |
| [frontend/src/utilities/ui_tools/mod.rs](/frontend/src/utilities/ui_tools/mod.rs) | Rust | 3 | 0 | 0 | 3 |
| [frontend/src/utilities/ui_tools/toasts.rs](/frontend/src/utilities/ui_tools/toasts.rs) | Rust | 270 | 91 | 46 | 407 |
| [frontend/src/utilities/update_tasks.rs](/frontend/src/utilities/update_tasks.rs) | Rust | 179 | 3 | 42 | 224 |
| [frontend/src/webworker.rs](/frontend/src/webworker.rs) | Rust | 67 | 27 | 21 | 115 |
| [websocket_server/.cargo/config.toml](/websocket_server/.cargo/config.toml) | TOML | 0 | 9 | 3 | 12 |
| [websocket_server/.env](/websocket_server/.env) | Properties | 1 | 0 | 0 | 1 |
| [websocket_server/Cargo.lock](/websocket_server/Cargo.lock) | TOML | 1,590 | 2 | 190 | 1,782 |
| [websocket_server/Cargo.toml](/websocket_server/Cargo.toml) | TOML | 15 | 2 | 4 | 21 |
| [websocket_server/Dockerfile](/websocket_server/Dockerfile) | Docker | 33 | 4 | 5 | 42 |
| [websocket_server/src/main.rs](/websocket_server/src/main.rs) | Rust | 245 | 53 | 35 | 333 |

[Summary](results.md) / Details / [Diff Summary](diff.md) / [Diff Details](diff-details.md)