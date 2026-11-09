import AVFAudio
import Foundation
#if canImport(Translation)
import Translation
#endif

private struct BridgeError: LocalizedError {
    let message: String
    init(_ message: String) { self.message = message }
    var errorDescription: String? { message }
}

private struct BridgeVoice: Codable {
    let id: String
    let name: String
    let accent: String
}

private final class BlockingResult<Value: Sendable>: @unchecked Sendable {
    private let lock = NSLock()
    private var value: Result<Value, Error>?

    func store(_ value: Result<Value, Error>) {
        lock.withLock { self.value = value }
    }

    func take() -> Result<Value, Error> {
        lock.withLock { value! }
    }
}

private func blocking<Value: Sendable>(
    timeout: TimeInterval,
    _ operation: @escaping @Sendable () async throws -> Value
) -> Result<Value, Error> {
    let semaphore = DispatchSemaphore(value: 0)
    let result = BlockingResult<Value>()
    let task = Task.detached(priority: .utility) {
        do {
            result.store(.success(try await operation()))
        } catch {
            result.store(.failure(error))
        }
        semaphore.signal()
    }
    guard semaphore.wait(timeout: .now() + timeout) == .success else {
        task.cancel()
        return .failure(BridgeError("On-device translation timed out."))
    }
    return result.take()
}

private func writeError(
    _ error: Error,
    to destination: UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?
) {
    destination?.pointee = strdup(
        (error as? LocalizedError)?.errorDescription ?? error.localizedDescription
    )
}

private let english = Locale.Language(identifier: "en")
private let chinese = Locale.Language(identifier: "zh-Hans")

/// 0 ready, 1 not installed, 2 unsupported macOS, 3 unsupported pair, 4 failed.
@_cdecl("hex_apple_translation_status")
public func hexAppleTranslationStatus(
    _ timeoutMs: UInt64,
    _ errorOutput: UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?
) -> Int32 {
    #if canImport(Translation)
    guard #available(macOS 26.0, *) else { return 2 }
    let result = blocking(timeout: TimeInterval(timeoutMs) / 1_000) {
        try await translationStatus()
    }
    switch result {
    case .success(let code):
        return code
    case .failure(let error):
        writeError(error, to: errorOutput)
        return 4
    }
    #else
    return 2
    #endif
}

#if canImport(Translation)
@available(macOS 26.0, *)
private func translationStatus() async throws -> Int32 {
    let availability: LanguageAvailability
    if #available(macOS 26.4, *) {
        availability = LanguageAvailability(preferredStrategy: .lowLatency)
    } else {
        availability = LanguageAvailability()
    }
    switch await availability.status(from: english, to: chinese) {
    case .unsupported:
        return 3
    case .supported:
        return 1
    case .installed:
        let session = makeSession()
        return await session.isReady ? 0 : 1
    @unknown default:
        return 4
    }
}

@available(macOS 26.0, *)
private func makeSession() -> TranslationSession {
    if #available(macOS 26.4, *) {
        return TranslationSession(
            installedSource: english,
            target: chinese,
            preferredStrategy: .lowLatency
        )
    }
    return TranslationSession(installedSource: english, target: chinese)
}
#endif

@_cdecl("hex_apple_translation_translate")
public func hexAppleTranslationTranslate(
    _ text: UnsafePointer<CChar>?,
    _ timeoutMs: UInt64,
    _ errorOutput: UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?
) -> UnsafeMutablePointer<CChar>? {
    #if canImport(Translation)
    guard #available(macOS 26.0, *), let text else {
        writeError(BridgeError("On-device translation needs macOS 26 or later."), to: errorOutput)
        return nil
    }
    let source = String(cString: text)
    let result = blocking(timeout: TimeInterval(timeoutMs) / 1_000) {
        let session = makeSession()
        guard await session.isReady else {
            throw BridgeError("English and Simplified Chinese are not installed.")
        }
        return try await session.translate(source).targetText
    }
    switch result {
    case .success(let translated):
        return strdup(translated)
    case .failure(let error):
        writeError(error, to: errorOutput)
        return nil
    }
    #else
    writeError(BridgeError("On-device translation needs macOS 26 or later."), to: errorOutput)
    return nil
    #endif
}

@_cdecl("hex_apple_translation_voices")
public func hexAppleTranslationVoices() -> UnsafeMutablePointer<CChar>? {
    let voices = AVSpeechSynthesisVoice.speechVoices().compactMap { voice -> BridgeVoice? in
        let language = voice.language.lowercased()
        let accent: String
        if language.hasPrefix("en-us") {
            accent = "us"
        } else if language.hasPrefix("en-gb") {
            accent = "gb"
        } else {
            return nil
        }
        return BridgeVoice(id: voice.identifier, name: voice.name, accent: accent)
    }
    guard let data = try? JSONEncoder().encode(voices),
          let json = String(data: data, encoding: .utf8)
    else {
        return strdup("[]")
    }
    return strdup(json)
}

private final class SpeechOwner: NSObject, AVSpeechSynthesizerDelegate, @unchecked Sendable {
    static let shared = SpeechOwner()
    let synthesizer = AVSpeechSynthesizer()
    private let lock = NSLock()
    private var commandSeq: UInt64 = 0
    private var latestToken: UInt64 = 0
    private var latestCanceled = false
    private var speakingFlag = false
    private var currentUtterance: AVSpeechUtterance?

    override private init() {
        super.init()
        synthesizer.delegate = self
    }

    var speaking: Bool {
        lock.withLock { speakingFlag }
    }

    func issueStart() -> UInt64 { issue() }
    func issueStop() -> UInt64 { issue() }

    func cancel(_ token: UInt64) {
        lock.withLock {
            if token == latestToken {
                latestCanceled = true
            }
        }
    }

    func runStart(token: UInt64, text: String, voiceId: String, slow: Bool) throws {
        guard live(token) else {
            throw BridgeError("Speech could not start.")
        }
        guard let voice = AVSpeechSynthesisVoice(identifier: voiceId) else {
            throw BridgeError("That English voice is not installed.")
        }
        let language = voice.language.lowercased()
        guard language.hasPrefix("en-us") || language.hasPrefix("en-gb") else {
            throw BridgeError("That English voice is not installed.")
        }
        guard live(token) else {
            throw BridgeError("Speech could not start.")
        }
        let utterance = AVSpeechUtterance(string: text)
        utterance.voice = voice
        utterance.rate = slow
            ? AVSpeechUtteranceDefaultSpeechRate * 0.5
            : AVSpeechUtteranceDefaultSpeechRate
        let accepted = lock.withLock { () -> Bool in
            guard liveLocked(token) else { return false }
            currentUtterance = utterance
            speakingFlag = true
            return true
        }
        guard accepted else {
            throw BridgeError("Speech could not start.")
        }
        guard live(token) else {
            dropIfCurrent(utterance)
            throw BridgeError("Speech could not start.")
        }
        synthesizer.stopSpeaking(at: .immediate)
        guard live(token) else {
            dropIfCurrent(utterance)
            throw BridgeError("Speech could not start.")
        }
        synthesizer.speak(utterance)
    }

    func runStop(token: UInt64) {
        let accepted = lock.withLock { () -> Bool in
            guard liveLocked(token) else { return false }
            speakingFlag = false
            currentUtterance = nil
            return true
        }
        guard accepted else { return }
        synthesizer.stopSpeaking(at: .immediate)
    }

    func speechSynthesizer(_: AVSpeechSynthesizer, didFinish utterance: AVSpeechUtterance) {
        dropIfCurrent(utterance)
    }

    func speechSynthesizer(_: AVSpeechSynthesizer, didCancel utterance: AVSpeechUtterance) {
        dropIfCurrent(utterance)
    }

    private func issue() -> UInt64 {
        lock.withLock {
            commandSeq += 1
            latestToken = commandSeq
            latestCanceled = false
            return commandSeq
        }
    }

    private func live(_ token: UInt64) -> Bool {
        lock.withLock { liveLocked(token) }
    }

    private func liveLocked(_ token: UInt64) -> Bool {
        token == latestToken && !latestCanceled
    }

    private func dropIfCurrent(_ utterance: AVSpeechUtterance) {
        lock.withLock {
            guard utterance === currentUtterance else { return }
            speakingFlag = false
            currentUtterance = nil
        }
    }
}

@_cdecl("hex_apple_translation_speak")
public func hexAppleTranslationSpeak(
    _ text: UnsafePointer<CChar>?,
    _ voiceId: UnsafePointer<CChar>?,
    _ slow: Bool,
    _ errorOutput: UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?
) -> Int32 {
    guard let text, let voiceId else {
        writeError(BridgeError("Missing speech text."), to: errorOutput)
        return -1
    }
    let spoken = String(cString: text)
    let identifier = String(cString: voiceId)
    let token = SpeechOwner.shared.issueStart()
    let result: Result<Void, Error>
    if Thread.isMainThread {
        result = Result {
            try SpeechOwner.shared.runStart(
                token: token, text: spoken, voiceId: identifier, slow: slow
            )
        }
    } else {
        let box = BlockingResult<Void>()
        let semaphore = DispatchSemaphore(value: 0)
        DispatchQueue.main.async {
            do {
                try SpeechOwner.shared.runStart(
                    token: token, text: spoken, voiceId: identifier, slow: slow
                )
                box.store(.success(()))
            } catch {
                box.store(.failure(error))
            }
            semaphore.signal()
        }
        if semaphore.wait(timeout: .now() + 2) == .success {
            result = box.take()
        } else {
            SpeechOwner.shared.cancel(token)
            result = .failure(BridgeError("Speech could not start."))
        }
    }
    switch result {
    case .success:
        return 0
    case .failure(let error):
        writeError(error, to: errorOutput)
        return -1
    }
}

@_cdecl("hex_apple_translation_stop_speech")
public func hexAppleTranslationStopSpeech() {
    let token = SpeechOwner.shared.issueStop()
    if Thread.isMainThread {
        SpeechOwner.shared.runStop(token: token)
    } else {
        DispatchQueue.main.async {
            SpeechOwner.shared.runStop(token: token)
        }
    }
}

@_cdecl("hex_apple_translation_is_speaking")
public func hexAppleTranslationIsSpeaking() -> Bool {
    SpeechOwner.shared.speaking
}

@_cdecl("hex_apple_translation_free_string")
public func hexAppleTranslationFreeString(_ string: UnsafeMutablePointer<CChar>?) {
    free(string)
}
