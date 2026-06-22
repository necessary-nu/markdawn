import Foundation
import UIKit
import MarkdawnFFI
#if canImport(Darwin)
import Darwin
#endif

public enum Markdawn {
  /// One-shot HTML rendering.
  public static func renderHTML(_ source: String) -> String {
    let p = HTMLParser()
    p.push(source)
    p.finish()
    return p.html
  }

  /// Streaming HTML parser. Push chunks; call `finish()` to flush trailing
  /// state; read `html` for an incremental snapshot.
  public final class HTMLParser {
    private var state: OpaquePointer?

    public init() {
      state = mw_state_new()
    }

    deinit {
      if let s = state { mw_state_free(s) }
    }

    @discardableResult
    public func push(_ chunk: String) -> Bool {
      guard let s = state else { return false }
      var ok = true
      chunk.withCString { cstr in
        let len = strlen(cstr)
        cstr.withMemoryRebound(to: UInt8.self, capacity: len) { ptr in
          ok = mw_state_push_chunk(s, ptr, len) == 0
        }
      }
      return ok
    }

    @discardableResult
    public func finish() -> Bool {
      guard let s = state else { return false }
      return mw_state_finish(s) == 0
    }

    public var html: String {
      guard let s = state, let ptr = mw_state_render(s) else { return "" }
      defer { mw_string_free(ptr) }
      return String(cString: ptr)
    }
  }

  /// Streaming UIKit renderer. Must be created on the main thread.
  /// `stackView` is a live view that updates in place as you push chunks.
  @MainActor
  public final class UIKitRenderer {
    private var state: OpaquePointer?

    public init() {
      state = mw_uikit_state_new()
      precondition(state != nil, "UIKitRenderer must be created on the main thread")
    }

    deinit {
      if let s = state { mw_uikit_state_free(s) }
    }

    @discardableResult
    public func push(_ chunk: String) -> Bool {
      guard let s = state else { return false }
      var ok = true
      chunk.withCString { cstr in
        let len = strlen(cstr)
        cstr.withMemoryRebound(to: UInt8.self, capacity: len) { ptr in
          ok = mw_uikit_state_push_chunk(s, ptr, len) == 0
        }
      }
      return ok
    }

    @discardableResult
    public func finish() -> Bool {
      guard let s = state else { return false }
      return mw_uikit_state_finish(s) == 0
    }

    /// Live `UIStackView`. Embed it in your view hierarchy; subsequent pushes
    /// mutate this same view tree in place.
    public var stackView: UIStackView {
      let raw = mw_uikit_state_stack_view(state)!
      return Unmanaged<UIStackView>.fromOpaque(raw).takeRetainedValue()
    }

    /// Snapshot of the entire document as a single `NSAttributedString`.
    public var attributedText: NSAttributedString {
      let raw = mw_uikit_state_attributed_text(state)!
      return Unmanaged<NSAttributedString>.fromOpaque(raw).takeRetainedValue()
    }
  }

  /// Load a tree-sitter grammar from a bundled plugin framework so subsequent
  /// renders highlight code blocks tagged with that language. Returns true on
  /// success. Idempotent — re-loading a language is a cheap no-op once
  /// arborium's grammar store has it.
  ///
  /// The corresponding `MarkdawnLang{Pascal}.framework` must be embedded in
  /// the app's bundle (add the SwiftPM product to your app target).
  @discardableResult
  public static func loadLanguage(_ name: String) -> Bool {
    let pascal = name.split(separator: "-").map { String($0).capitalized }.joined()
    let frameworkName = "MarkdawnLang\(pascal)"

    // Look in the app's PrivateFrameworks first (where SwiftPM-embedded
    // frameworks live for app targets) and fall back to Bundle.main itself
    // (where xctest puts them).
    let candidates: [String] = {
      var paths: [String] = []
      if let p = Bundle.main.privateFrameworksPath {
        paths.append("\(p)/\(frameworkName).framework/\(frameworkName)")
      }
      paths.append("\(Bundle.main.bundlePath)/Frameworks/\(frameworkName).framework/\(frameworkName)")
      // Last-ditch: maybe the framework is part of the loaded bundle set
      // already (test bundles sometimes put it directly in the test bundle).
      for bundle in Bundle.allBundles + Bundle.allFrameworks {
        if let exe = bundle.executablePath, exe.hasSuffix("/\(frameworkName)") {
          paths.append(exe)
        }
      }
      return paths
    }()

    var handle: UnsafeMutableRawPointer?
    for path in candidates {
      if let h = dlopen(path, RTLD_NOW) {
        handle = h
        break
      }
    }
    guard let handle else { return false }

    let symbolName = "mw_lang_\(name.replacingOccurrences(of: "-", with: "_"))_init"
    guard let initSym = dlsym(handle, symbolName) else { return false }

    typealias RegisterFn = @convention(c) (
      UnsafePointer<CChar>?, Int,
      UnsafeRawPointer?,
      UnsafePointer<CChar>?, Int,
      UnsafePointer<CChar>?, Int,
      UnsafePointer<CChar>?, Int
    ) -> UInt32
    typealias InitFn = @convention(c) (RegisterFn) -> UInt32

    let initFn = unsafeBitCast(initSym, to: InitFn.self)
    return initFn(mw_register_grammar) == 0
  }
}
