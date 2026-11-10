import Foundation
import Security

/// How the broadcast extension hands screen pictures to the app: a local connection that opens
/// with a secret token, then each picture as a `Header`, its Y plane and interleaved UV plane.
/// The port and token travel through a Keychain group only these two targets can read.
enum ScreenLink {
    static let magic: UInt32 = 0x464C_5853
    static let maximumPlane = 16 * 1024 * 1024
    static let tokenLength = 32

    struct Header {
        var magic: UInt32 = ScreenLink.magic
        var width: UInt32
        var height: UInt32
        var yStride: UInt32
        var uvStride: UInt32
        var rotation: UInt32
        var yLength: UInt32
        var uvLength: UInt32
    }

    struct Endpoint: Codable {
        let port: UInt16
        let token: Data
    }

    private static var query: [CFString: Any] {
        var query: [CFString: Any] = [
            kSecClass: kSecClassGenericPassword,
            kSecAttrService: "flexspeak.broadcast",
            kSecAttrAccount: "endpoint",
        ]
        if let group = Bundle.main.object(forInfoDictionaryKey: "FlexspeakKeychainGroup") as? String {
            query[kSecAttrAccessGroup] = group
        }
        return query
    }

    /// Makes the endpoint available to the extension. False when the Keychain refuses it.
    static func publish(_ endpoint: Endpoint) -> Bool {
        guard let data = try? JSONEncoder().encode(endpoint) else { return false }
        SecItemDelete(query as CFDictionary)
        var item = query
        item[kSecValueData] = data
        item[kSecAttrAccessible] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
        return SecItemAdd(item as CFDictionary, nil) == errSecSuccess
    }

    static func withdraw() {
        SecItemDelete(query as CFDictionary)
    }

    static func endpoint() -> Endpoint? {
        var search = query
        search[kSecReturnData] = true
        search[kSecMatchLimit] = kSecMatchLimitOne
        var result: CFTypeRef?
        guard SecItemCopyMatching(search as CFDictionary, &result) == errSecSuccess,
              let data = result as? Data else { return nil }
        return try? JSONDecoder().decode(Endpoint.self, from: data)
    }

    /// The loopback address for `port`, never reachable from another device.
    static func address(port: UInt16) -> sockaddr_in {
        var address = sockaddr_in()
        address.sin_len = UInt8(MemoryLayout<sockaddr_in>.size)
        address.sin_family = sa_family_t(AF_INET)
        address.sin_port = port.bigEndian
        address.sin_addr.s_addr = in_addr_t(INADDR_LOOPBACK).bigEndian
        return address
    }
}
