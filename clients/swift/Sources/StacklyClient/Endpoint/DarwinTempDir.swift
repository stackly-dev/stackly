import Darwin

/// The user's private temporary folder, the per user 0700 folder macOS
/// creates under `/var/folders`, from `confstr(_CS_DARWIN_USER_TEMP_DIR)`.
public enum DarwinTempDir {
    /// The raw bytes of the folder path, without the trailing NUL.
    ///
    /// - Throws: ``RootOverrideProblem/tempDirUnavailable`` when macOS reports
    ///   no folder or a relative one.
    public static func current() throws(RootOverrideProblem) -> [UInt8] {
        let needed = confstr(_CS_DARWIN_USER_TEMP_DIR, nil, 0)
        guard needed > 0 else { throw .tempDirUnavailable }
        var buffer = [CChar](repeating: 0, count: needed)
        let written = buffer.withUnsafeMutableBufferPointer { pointer in
            confstr(_CS_DARWIN_USER_TEMP_DIR, pointer.baseAddress, pointer.count)
        }
        guard written > 0, written <= needed else { throw .tempDirUnavailable }
        let bytes = buffer.prefix { $0 != 0 }.map { UInt8(bitPattern: $0) }
        guard bytes.first == UInt8(ascii: "/") else { throw .tempDirUnavailable }
        return bytes
    }
}
