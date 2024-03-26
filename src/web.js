export const chooseDir = async () => {
    return await window.showDirectoryPicker();
}

export const readBytes = async (dir, fileName) => {
    const handle = await dir.getFileHandle(fileName);
    const file = await handle.getFile();
    return await file.arrayBuffer();
}

export const readCell = async (dir, hierarchy, fileName) => {
    const dirHandle = await dir.getDirectoryHandle(hierarchy);
    return readBytes(dirHandle, fileName);
}