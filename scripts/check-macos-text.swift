#!/usr/bin/env swift
// Inspect only the app-window screenshot supplied on the command line.
import Foundation
import Vision

if CommandLine.arguments.count != 2 {
    fatalError("Usage: swift scripts/check-macos-text.swift <app-window.png>")
}
let request = VNRecognizeTextRequest()
request.recognitionLevel = .accurate
request.usesLanguageCorrection = false
let handler = VNImageRequestHandler(url: URL(fileURLWithPath: CommandLine.arguments[1]))
try handler.perform([request])
let text = (request.results ?? []).compactMap { $0.topCandidates(1).first?.string }.joined(separator: " ").lowercased()
let missing = ["rules", "refresh"].filter { !text.contains($0) }
if !missing.isEmpty {
    fputs("Missing rendered app labels: \(missing.joined(separator: ", ")). OCR: \(text)\n", stderr)
    exit(1)
}
print("PASS: visible Rules and Refresh labels in app-window capture")
