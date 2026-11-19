import XCTest
@testable import Duck

/// The pet-detect wire's pure half: 48 kHz Float32 in, a 16 kHz mono i16
/// one-second window out, base64 over the exact bytes the core will decode.
/// The tap/queue plumbing needs an audio engine and belongs to the phone;
/// the reducer and the encoding are pinned here.
final class PetAudioTests: XCTestCase {
    func testFortyEightKhzBecomesSixteenKhzOneSecond() {
        // One second of 48 kHz input yields exactly 16 000 samples.
        let input = [Float](repeating: 0.25, count: 48_000)
        let reduced = PetPCM.downsample(input, sourceRate: 48_000)
        XCTAssertEqual(reduced.count, 16_000)
        XCTAssertEqual(PetPCM.windowSamples, 16_000)
        XCTAssertEqual(reduced.count, PetPCM.windowSamples)
        // 0.25 * 32767 = 8191.75, rounded.
        XCTAssertTrue(reduced.allSatisfy { $0 == 8_192 })
        XCTAssertEqual(PetPCM.outputCount(frames: 48_000, sourceRate: 48_000), 16_000)
    }

    func testTheTapBufferLengthIsPreservedByTheRatio() {
        // A 4096-frame tap buffer at 48 kHz is ~85 ms, i.e. 1365 samples.
        let count = PetPCM.outputCount(frames: 4_096, sourceRate: 48_000)
        XCTAssertEqual(count, 1_365)
        let reduced = PetPCM.downsample(
            [Float](repeating: 0, count: 4_096),
            sourceRate: 48_000
        )
        XCTAssertEqual(reduced.count, count)
    }

    func testFortyFourPointOneKhzAlsoFitsOneSecond() {
        let reduced = PetPCM.downsample(
            [Float](repeating: 0, count: 44_100),
            sourceRate: 44_100
        )
        XCTAssertEqual(reduced.count, 16_000)
        XCTAssertEqual(PetPCM.outputCount(frames: 44_100, sourceRate: 44_100), 16_000)
    }

    /// The converter clamps full-scale positives and negatives instead of
    /// trapping on an Int16 conversion.
    func testBoundarySamplesClamp() {
        let reduced = PetPCM.downsample([2.0, -2.0, 1.0, -1.0], sourceRate: 16_000)
        XCTAssertEqual(reduced, [32_767, -32_767, 32_767, -32_767])
    }

    func testDegenerateInputsYieldNothing() {
        XCTAssertEqual(PetPCM.downsample([], sourceRate: 48_000), [])
        XCTAssertEqual(PetPCM.outputCount(frames: 0, sourceRate: 48_000), 0)
        // Upsampling would invent detail the microphone never heard.
        XCTAssertEqual(
            PetPCM.downsample([Float](repeating: 0.5, count: 8_000), sourceRate: 8_000),
            []
        )
    }

    /// The bytes really are little-endian i16 and survive base64 untouched.
    func testBase64RoundTripsTheSamples() {
        let samples: [Int16] = [0, 1, -1, 32_767, -32_768, 1_234, -4_321]
        let encoded = PetPCM.encodeBase64(samples)
        guard let data = Data(base64Encoded: encoded) else {
            XCTFail("the shell produced something that is not base64")
            return
        }
        XCTAssertEqual(data.count, samples.count * MemoryLayout<Int16>.size)
        let bytes = [UInt8](data)
        var decoded: [Int16] = []
        var index = 0
        while index + 1 < bytes.count {
            let bits = UInt16(bytes[index]) | (UInt16(bytes[index + 1]) << 8)
            decoded.append(Int16(bitPattern: bits))
            index += 2
        }
        XCTAssertEqual(decoded, samples)
    }

    /// One wire window is 32 000 PCM bytes (~42 KB of base64), which is the
    /// ticket's 42 KB/s local-loopback budget.
    func testOneSecondPayloadIsThirtyTwoKilobytes() {
        let silence = [Int16](repeating: 0, count: PetPCM.windowSamples)
        XCTAssertEqual(silence.count * MemoryLayout<Int16>.size, 32_000)
        let encoded = PetPCM.encodeBase64(silence)
        XCTAssertEqual(Data(base64Encoded: encoded)?.count ?? -1, 32_000)
        XCTAssertGreaterThan(encoded.count, 42_000)
        XCTAssertLessThan(encoded.count, 43_000)
    }

    /// J1's `POST /sensor` routes on `sensor` and reads `value.pcm16`; a bare
    /// body with the audio at the top level is refused as "missing sensor".
    /// This pins the envelope the iOS sender actually writes.
    func testPCMRidesInsideTheMicrophoneReadingEnvelope() {
        let body = PetPCM.postBody(
            pcm16: "AQD+/w==",
            rms: 0.25,
            peakHz: 440,
            tickMS: 1_700_000_000_000
        )
        XCTAssertEqual(body["sensor"] as? String ?? "", "microphone")
        XCTAssertEqual(body["mode"] as? String ?? "", "twin")
        let value = body["value"] as? [String: Any]
        XCTAssertEqual(value?["pcm16"] as? String ?? "", "AQD+/w==")
        XCTAssertEqual(value?["rate"] as? Int ?? -1, 16_000)
        XCTAssertEqual(value?["rms"] as? Double ?? -1, 0.25)
        XCTAssertEqual(value?["peak_hz"] as? Double ?? -1, 440)
        // The core parses this exact JSON, not the Swift dictionary.
        let data = try? JSONSerialization.data(withJSONObject: body)
        let parsed = data.flatMap { raw -> [String: Any]? in
            guard let object = try? JSONSerialization.jsonObject(with: raw),
                  let dictionary = object as? [String: Any]
            else { return nil }
            return dictionary
        }
        XCTAssertEqual(parsed?["sensor"] as? String ?? "", "microphone")
        XCTAssertEqual(
            (parsed?["value"] as? [String: Any])?["pcm16"] as? String ?? "",
            "AQD+/w=="
        )
    }
}