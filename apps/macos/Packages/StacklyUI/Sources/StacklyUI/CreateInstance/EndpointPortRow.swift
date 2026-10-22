import StacklyClient
import SwiftUI

/// One endpoint of the draft (spec 0020, *Port*): the label "Port · {name}", Fixed or
/// Automatic, the port or the note that it comes at start, and under it what the preflight
/// found for the endpoint.
///
/// A taken Fixed port says so with a symbol and words and offers the free port the daemon
/// found; the offer is applied only by the button, never on its own.
struct EndpointPortRow: View {
    let model: CreateInstanceModel
    let endpoint: EndpointDraft

    var body: some View {
        VStack(alignment: .leading, spacing: Spacing.s6) {
            LabeledContent("Port · \(endpoint.name)") {
                HStack(spacing: Spacing.s8) {
                    Picker("Port type, \(endpoint.name)", selection: mode) {
                        ForEach([PortMode.fixed, .automatic], id: \.self) { Text($0.title) }
                    }
                    .labelsHidden()
                    .pickerStyle(.segmented)
                    .fixedSize()
                    .accessibilityIdentifier(
                        AccessibilityID.CreateInstance.portMode(endpoint.name))
                    switch endpoint.mode {
                    case .fixed:
                        TextField("Port", text: portText)
                            .labelsHidden()
                            .accessibilityLabel("Port, \(endpoint.name)")
                            .multilineTextAlignment(.trailing)
                            .monospacedDigit()
                            .frame(width: 84)
                            .accessibilityIdentifier(
                                AccessibilityID.CreateInstance.port(endpoint.name))
                    case .automatic:
                        Text("Assigned when the instance starts")
                            .font(.callout)
                            .foregroundStyle(StacklyColor.textSecondary)
                    }
                }
            }
            note
        }
    }

    private var mode: Binding<PortMode> {
        Binding(
            get: { endpoint.mode },
            set: { model.setMode($0, of: endpoint.name) })
    }

    private var portText: Binding<String> {
        Binding(
            get: { endpoint.portText },
            set: { model.setPortText($0, of: endpoint.name) })
    }

    @ViewBuilder private var note: some View {
        if endpoint.hasInvalidPort {
            Label(EndpointDraft.invalidPortMessage, systemImage: Tone.critical.symbol)
                .font(.callout)
                .foregroundStyle(Tone.critical.text)
        } else if let check = model.check(forEndpoint: endpoint.name), endpoint.mode == .fixed {
            switch check.status {
            case .error:
                VStack(alignment: .leading, spacing: Spacing.s4) {
                    Label(check.detail, systemImage: Tone.critical.symbol)
                    Text("Fixed ports are never changed silently.")
                        .foregroundStyle(StacklyColor.textSecondary)
                    if let suggested = check.suggestedPort {
                        Button("Use \(String(suggested))") {
                            model.usePort(suggested, for: endpoint.name)
                        }
                        .accessibilityLabel("Use port \(String(suggested))")
                        .accessibilityIdentifier(
                            AccessibilityID.CreateInstance.usePort(endpoint.name))
                    }
                }
                .font(.callout)
                .foregroundStyle(Tone.critical.text)
            case .caution:
                Label(check.detail, systemImage: Tone.caution.symbol)
                    .font(.callout)
                    .foregroundStyle(Tone.caution.text)
            case .ok, .unrecognized:
                EmptyView()
            }
        }
    }
}
