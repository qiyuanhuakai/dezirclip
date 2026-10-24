/** Remove Windows extended-length syntax only from human-readable paths. */
export const displayPath = (path: string): string => {
    if (/^\\\\\?\\UNC\\/i.test(path)) return `\\\\${path.slice(8)}`;
    if (/^\\\\\?\\[a-z]:\\/i.test(path)) return path.slice(4);
    return path;
};
