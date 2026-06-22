import XCTest
import UIKit
@testable import Markdawn

final class MarkdawnTests: XCTestCase {
  func testHTML() {
    let html = Markdawn.renderHTML("# hello\n")
    XCTAssertTrue(html.contains("<h1"))
    XCTAssertTrue(html.contains("hello"))
  }

  func testHTMLStreaming() {
    let p = Markdawn.HTMLParser()
    p.push("# strea")
    p.push("ming\n\nparagraph\n")
    p.finish()
    XCTAssertTrue(p.html.contains("<h1"))
    XCTAssertTrue(p.html.contains("streaming"))
    XCTAssertTrue(p.html.contains("<p>paragraph"))
  }

  @MainActor
  func testUIKitStackView() {
    let r = Markdawn.UIKitRenderer()
    r.push("# heading\n\nparagraph\n")
    r.finish()
    XCTAssertGreaterThan(r.stackView.arrangedSubviews.count, 0)
  }

  @MainActor
  func testUIKitStreaming() {
    let r = Markdawn.UIKitRenderer()
    r.push("# strea")
    r.push("ming\n\n")
    r.push("paragraph\n")
    r.finish()
    XCTAssertGreaterThan(r.stackView.arrangedSubviews.count, 0)
  }

  @MainActor
  func testLoadLanguage() {
    // The test target depends on MarkdawnLangRust, so its framework is
    // embedded alongside the test bundle. After loadLanguage("rust"),
    // subsequent renders should highlight rust code blocks.
    XCTAssertTrue(Markdawn.loadLanguage("rust"), "rust grammar should load")

    let r = Markdawn.UIKitRenderer()
    r.push("```rust\nfn main() { println!(\"hi\"); }\n```\n")
    r.finish()
    XCTAssertGreaterThan(r.stackView.arrangedSubviews.count, 0)

    // Loading again should be a no-op success.
    XCTAssertTrue(Markdawn.loadLanguage("rust"))
  }
}
