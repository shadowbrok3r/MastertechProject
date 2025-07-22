This is a Rust based repository for assisting Computer Repair Technicians with computer diagnostics, making recommendations on upgrades for client's computers, task management, etc. Please follow these guidelines when contributing:

## Code Standards


## Repository Structure
- `Mastertech4.0/`: Main desktop application, which has two modes: terminal mode, and gui(egui) mode
- `MtechServer2.0/`: Website version of Mastertech4.0, compiled to web assembly
- `websocket_server2/`: Websocket server for remote control of Mastertech4.0 egui mode -> Mastertech4.0 terminal mode, or terminal mode -> terminal mode remote control
- `displays/`: All of the shared egui elements / logic between Mastertech4.0 and MtechServer2.0
- `database/`: All surrealdb database logic, type structures, api calls, etc that are shared between Mastertech4.0 and MtechServer2.0

## Key Guidelines
1. Follow Rust best practices and idiomatic patterns
2. Maintain existing code structure and organization
3. Make sure to understand the logic / changes for the latest cargo crate versions
4. Document public APIs and complex logic. Suggest changes to the `README.md` folder when appropriate