self.onmessage = async (event) => {
    const [mainJS, module, memory, id, onDonePtr] = event.data;
    const {default: init, executeFnOnce, executeFn} = await import(mainJS);
    await init(module, memory);

    console.log("Worker", id, "started");

    self.onmessage = async (event) => {
        if (typeof event.data === "string" && event.data === "terminate") {
            console.log("Closing", id);
            self.close();
            return;
        }

        const [fnPtr, params, onDonePtr] = event.data;
        await executeFnOnce(fnPtr, params);
        executeFn(onDonePtr, [id]);
    }

    executeFn(onDonePtr, [id]);
}