// Node-only bundle replacement for the optional Bun storage backend.
export const constants = Object.freeze({});
export class Database {
    constructor() {
        throw new Error("bun_sqlite_unavailable");
    }
}
