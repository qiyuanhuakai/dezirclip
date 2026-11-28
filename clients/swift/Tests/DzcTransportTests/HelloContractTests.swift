import DzcProtocol
import Foundation
import Testing

private let response = """
{"session_id":"launcher-session","protocol_version":{"major":1,"minor":0},"core_version":"0.2.0",
 "permissions":["history_read"],"state":{"service_instance_id":"instance","history_revision":3,
 "privacy_epoch":7,"event_sequence":11},"max_frame_bytes":1048576,"future_property":true}
"""

@Test func helloAllowsFutureFieldsAndReadsNegotiatedValues() throws {
    let hello = try JSONDecoder().decode(HelloResult.self, from: Data(response.utf8))
    #expect(hello.protocolVersion.major == 1)
    #expect(hello.permissions == [.historyRead])
    #expect(hello.state.serviceInstanceId == "instance")
    #expect(hello.state.privacyEpoch == 7)
    #expect(hello.maxFrameBytes == UInt32(WireLimits.maximumFrameBytes))
}

@Test func helloRejectsUnknownPermissionsAndOutOfRangeIntegers() {
    let decoder = JSONDecoder()
    for (original, invalid) in [
        ("history_read", "future_permission"),
        (#""privacy_epoch":7"#, #""privacy_epoch":9007199254740992"#),
        (#""history_revision":3"#, #""history_revision":-1"#),
        (#""max_frame_bytes":1048576"#, #""max_frame_bytes":1048577"#),
        (#""major":1"#, #""major":65536"#),
    ] {
        let malformed = response.replacingOccurrences(of: original, with: invalid)
        #expect(throws: (any Error).self) {
            try decoder.decode(HelloResult.self, from: Data(malformed.utf8))
        }
    }
}

@Test func optionalGrantAllowsOmissionAndRejectsNullOrInvalidTokens() throws {
    let start = """
    {"protocol_version":{"major":1,"minor":0},"client_version":"0.2.0","client_role":"cli",
     "capabilities":[],"required_capabilities":[]
    """
    let decoder = JSONDecoder()
    #expect(try decoder.decode(HelloParams.self, from: Data((start + "}").utf8)).grant == nil)
    for invalid in ["null", "\"bad-token\"", "\"" + String(repeating: "a", count: 64) + "\\n\""] {
        let input = start + ",\"grant\":" + invalid + "}"
        #expect(throws: (any Error).self) {
            try decoder.decode(HelloParams.self, from: Data(input.utf8))
        }
    }
}
