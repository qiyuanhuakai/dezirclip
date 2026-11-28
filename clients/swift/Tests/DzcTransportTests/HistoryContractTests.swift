import DzcProtocol
import Foundation
import Testing

@Test func eventPayloadPreservesNegativeIdentifiersAndRejectsUnknownFields() throws {
    let decoder = JSONDecoder()
    let valid = Data(#"{"entry_id":-1,"status":"usage_changed"}"#.utf8)
    let event = try decoder.decode(EventData.self, from: valid)
    #expect(event.entryId == -1)
    #expect(try decoder.decode(EventData.self, from: JSONEncoder().encode(event)) == event)
    #expect(try decoder.decode(EventData.self, from: Data("{}".utf8)).entryId == nil)
    for invalid in [#"{"entry_id":0}"#, #"{"entry_id":null}"#, #"{"future_field":1}"#] {
        #expect(throws: (any Error).self) {
            try decoder.decode(EventData.self, from: Data(invalid.utf8))
        }
    }
}

@Test func historyAndSnapshotObserveSchemaBounds() throws {
    let page = try DesktopPageParams(limit: 150, offset: 1_000_000, query: "剪贴板")
    #expect(try JSONDecoder().decode(DesktopPageParams.self, from: JSONEncoder().encode(page)) == page)
    #expect(throws: (any Error).self) { try DesktopPageParams(limit: 151, offset: 0) }
    let state = try RuntimeState(serviceInstanceId: "local-host", historyRevision: 10, privacyEpoch: 2, eventSequence: 9)
    let opaqueID = String(repeating: "a", count: 32)
    let snapshot = try BodySnapshot(snapshotId: opaqueID, totalBytes: 33_554_432, state: state)
    #expect(try JSONDecoder().decode(BodySnapshot.self, from: JSONEncoder().encode(snapshot)) == snapshot)
    #expect(throws: (any Error).self) {
        try BodySnapshot(snapshotId: opaqueID, totalBytes: 33_554_433, state: state)
    }
    #expect(throws: (any Error).self) {
        try BodySnapshot(snapshotId: opaqueID, totalBytes: 0, state: state, inlineJson: "")
    }
}

@Test func clipboardCopyRequiresAnInstanceAndNonzeroIdentifier() throws {
    let request = try DesktopCopyParams(id: -4, serviceInstanceId: "local-host", expectedEpoch: 2, reveal: false, paste: false)
    let bytes = try JSONEncoder().encode(request)
    let fields = try #require(JSONSerialization.jsonObject(with: bytes) as? [String: Any])
    #expect(fields["service_instance_id"] as? String == "local-host")
    #expect(fields["paste"] as? Bool == false)
    #expect(fields["expected_paste_target"] == nil)
    #expect(throws: (any Error).self) {
        try DesktopCopyParams(id: -4, serviceInstanceId: "", expectedEpoch: 2, reveal: false, paste: false)
    }
    #expect(throws: (any Error).self) {
        try DesktopCopyParams(id: 0, serviceInstanceId: "local-host", expectedEpoch: 2, reveal: false, paste: false)
    }
}
