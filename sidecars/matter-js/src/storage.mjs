import {
    MemoryStorageDriver,
    MockStorageService,
    StorageError,
    fromJson,
    toJson,
} from "@matter/main";

class RustStorageDriver extends MemoryStorageDriver {
    #bridge;
    #handle;
    #persistQueue = Promise.resolve();
    #revision;

    static async open(namespace, bridge) {
        const handle = `matter/storage/${namespace}`;
        const record = await bridge.get(handle);
        let store = {};
        let revision;
        if (record !== undefined) {
            const encoded = Buffer.from(record.value).toString("utf8");
            const decoded = fromJson(encoded);
            if (decoded === null || typeof decoded !== "object" || Array.isArray(decoded)) {
                throw new StorageError("rust_storage_invalid");
            }
            store = decoded;
            revision = record.revision;
        }
        return new RustStorageDriver(store, bridge, handle, revision);
    }

    constructor(store, bridge, handle, revision) {
        super(store);
        this.#bridge = bridge;
        this.#handle = handle;
        this.#revision = revision;
    }

    async #persist() {
        const value = Buffer.from(toJson(this.data));
        const persist = async () => {
            this.#revision =
                this.#revision === undefined
                    ? await this.#bridge.put(this.#handle, value)
                    : await this.#bridge.compareAndSwap(this.#handle, this.#revision, value);
        };
        const result = this.#persistQueue.then(persist, persist);
        this.#persistQueue = result.catch(() => {});
        await result;
    }

    async set(contexts, keyOrValues, value) {
        super.set(contexts, keyOrValues, value);
        await this.#persist();
    }

    async delete(contexts, key) {
        super.delete(contexts, key);
        await this.#persist();
    }

    async clearAll(contexts) {
        super.clearAll(contexts);
        await this.#persist();
    }
}

export const installRustStorage = (environment, bridge) => {
    new MockStorageService(environment, namespace => RustStorageDriver.open(namespace, bridge));
};
