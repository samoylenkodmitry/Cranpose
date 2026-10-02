import UIKit
import CranposeBindings

/// Creates one child for a registered native slot.
@MainActor public protocol NativeViewFactory {
    /// Creates a child whose events are delivered to the owning Rust slot.
    func create(emit: @escaping (String) -> Void) -> any NativeViewChild
}

/// Lifecycle shared by all native child factories.
@MainActor public protocol NativeViewChild: AnyObject {
    /// Platform view displayed above the Cranpose image.
    var view: UIView { get }
    /// Applies changed slot configuration.
    func update(_ value: String) throws
    /// Updates attachment and visibility state.
    func setVisible(_ visible: Bool)
    /// Releases platform resources.
    func dispose()
}

public extension NativeViewChild {
    func setVisible(_ visible: Bool) { view.isHidden = !visible }
}

@MainActor final class NativeViewRegistry {
    private final class Entry {
        let id: UInt64
        let kind: String
        let child: any NativeViewChild
        var seen = false
        init(id: UInt64, kind: String, child: any NativeViewChild) {
            self.id = id
            self.kind = kind
            self.child = child
        }
    }
    private unowned let parent: UIView
    private let factories: [String: any NativeViewFactory]
    private let emit: (UInt64, String) -> Void
    private var children: [Entry] = []
    private var visible = true

    init(parent: UIView, factories: [String: any NativeViewFactory], emit: @escaping (UInt64, String) -> Void) {
        self.parent = parent
        self.factories = factories
        self.emit = emit
    }

    func reconcile(_ slots: [NativeSlot]) throws {
        for slot in slots where factories[slot.kind] == nil {
            throw NSError(domain: "Cranpose", code: 1,
                userInfo: [NSLocalizedDescriptionKey: "Unknown native view factory: \(slot.kind)"])
        }
        for entry in children { entry.seen = false }
        for slot in slots {
            if let index = children.firstIndex(where: { $0.id == slot.id && $0.kind != slot.kind }) {
                remove(at: index)
            }
            let entry: Entry
            if let existing = children.first(where: { $0.id == slot.id }) { entry = existing }
            else {
                guard let factory = factories[slot.kind] else { continue }
                entry = Entry(id: slot.id, kind: slot.kind, child: factory.create { [emit] in emit(slot.id, $0) })
                children.append(entry)
                parent.addSubview(entry.child.view)
                entry.child.setVisible(visible)
            }
            entry.seen = true
            let bounds = CGRect(x: CGFloat(slot.x), y: CGFloat(slot.y), width: CGFloat(slot.width), height: CGFloat(slot.height))
            if entry.child.view.frame != bounds { entry.child.view.frame = bounds }
            try entry.child.update(slot.value)
            parent.bringSubviewToFront(entry.child.view)
        }
        var index = 0
        while index < children.count {
            if children[index].seen { index += 1 } else { remove(at: index) }
        }
    }

    func setVisible(_ value: Bool) {
        visible = value
        for entry in children { entry.child.setVisible(value) }
    }

    private func remove(at index: Int) {
        children[index].child.view.removeFromSuperview()
        children[index].child.dispose()
        children.swapAt(index, children.count - 1)
        children.removeLast()
    }

    func close() {
        while !children.isEmpty { remove(at: children.count - 1) }
    }
}
