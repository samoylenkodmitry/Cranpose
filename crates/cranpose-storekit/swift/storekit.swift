// StoreKit 2 bridge for `cranpose-storekit`.
//
// StoreKit 2 is Swift-only — `Product`, `Transaction`, `Transaction.updates`
// and the on-device JWS verification have no Objective-C surface, so this file
// is the whole reason the crate compiles Swift at all. Everything here is a
// thin translation layer: no policy, no caching decisions, no UI.
//
// The Rust side owns all state. Swift pushes snapshots through one C callback
// and never answers questions synchronously, because every StoreKit call is
// async and may block on the network or on a payment sheet.
//
// Threading: StoreKit callbacks arrive on Swift concurrency executors, i.e.
// arbitrary threads. The callback into Rust is therefore made from arbitrary
// threads and the Rust side guards its state with a mutex. Strings handed to
// the callback are valid only for the duration of the call — Rust copies them.

import Foundation
import StoreKit

// MARK: - C ABI

/// Message kinds pushed to Rust. A snapshot is `begin`, then any number of
/// `product`/`owned` rows, then `phase`, which commits it atomically.
private enum Kind: Int32 {
    case begin = 0
    case product = 1
    case owned = 2
    case phase = 3
    case event = 4
    case busy = 5
}

/// Mirrors `cranpose_storekit::ffi::StorePhaseCode`.
private enum PhaseCode: Int32 {
    case unavailable = 0
    case connecting = 1
    case ready = 2
    case blocked = 3
}

/// Mirrors `cranpose_storekit::ffi::EventCode`.
private enum EventCode: Int32 {
    case purchased = 0
    case cancelled = 1
    case pending = 2
    case failed = 3
    case restored = 4
}

/// `(ctx, kind, arg0, arg1, a, b, c, d)`.
public typealias CranposeStoreCallback = @convention(c) (
    UnsafeMutableRawPointer?,
    Int32,
    Int32,
    Int32,
    UnsafePointer<CChar>?,
    UnsafePointer<CChar>?,
    UnsafePointer<CChar>?,
    UnsafePointer<CChar>?
) -> Void

/// Callback + context, set once by `cranpose_storekit_start`.
///
/// `@unchecked Sendable`: the payload is a C function pointer and an opaque
/// pointer that Rust guarantees outlives the process (it points at a `static`).
private final class Sink: @unchecked Sendable {
    let callback: CranposeStoreCallback
    let ctx: UnsafeMutableRawPointer?

    init(callback: @escaping CranposeStoreCallback, ctx: UnsafeMutableRawPointer?) {
        self.callback = callback
        self.ctx = ctx
    }

    func send(
        _ kind: Kind,
        _ arg0: Int32 = 0,
        _ arg1: Int32 = 0,
        _ a: String? = nil,
        _ b: String? = nil,
        _ c: String? = nil,
        _ d: String? = nil
    ) {
        // Nested `withCString` keeps every buffer alive across the call. Four
        // levels is the whole product row; nothing needs more.
        withOptionalCString(a) { pa in
            withOptionalCString(b) { pb in
                withOptionalCString(c) { pc in
                    withOptionalCString(d) { pd in
                        callback(ctx, kind.rawValue, arg0, arg1, pa, pb, pc, pd)
                    }
                }
            }
        }
    }
}

private func withOptionalCString<R>(
    _ value: String?,
    _ body: (UnsafePointer<CChar>?) -> R
) -> R {
    guard let value else { return body(nil) }
    return value.withCString { body($0) }
}

// MARK: - Bridge state

/// Process-wide bridge state. Guarded by `lock`; every field is touched from
/// StoreKit's executors as well as from the caller's thread.
private final class BridgeState: @unchecked Sendable {
    private let lock = NSLock()
    private var _sink: Sink?
    private var _productIds: [String] = []
    private var _listenerActive = false
    private var _listenerGeneration: UInt64 = 0

    static let shared = BridgeState()

    var sink: Sink? {
        lock.lock()
        defer { lock.unlock() }
        return _sink
    }

    var productIds: [String] {
        lock.lock()
        defer { lock.unlock() }
        return _productIds
    }

    func start(sink: Sink, productIds: [String]) -> UInt64? {
        lock.lock()
        defer { lock.unlock() }
        _sink = sink
        _productIds = productIds
        let generation: UInt64?
        if !_listenerActive {
            _listenerGeneration &+= 1
            generation = _listenerGeneration
            _listenerActive = true
        } else {
            generation = nil
        }
        return generation
    }

    func listenerEnded(_ generation: UInt64) {
        lock.lock()
        if _listenerGeneration == generation {
            _listenerActive = false
        }
        lock.unlock()
    }

    var listenerActive: Bool {
        lock.lock()
        defer { lock.unlock() }
        return _listenerActive
    }
}

// MARK: - Entry points

/// Connect to the App Store and begin watching for transactions.
///
/// `productIdsJoined` is a newline-separated list — product ids are
/// `[A-Za-z0-9._-]`, so a newline cannot occur inside one and no escaping or
/// JSON parser is needed on either side.
///
/// Safe to call again after the transaction listener exits.
@_cdecl("cranpose_storekit_start")
public func cranpose_storekit_start(
    _ productIdsJoined: UnsafePointer<CChar>,
    _ ctx: UnsafeMutableRawPointer?,
    _ callback: @escaping CranposeStoreCallback
) {
    let ids = String(cString: productIdsJoined)
        .split(separator: "\n")
        .map(String.init)
        .filter { !$0.isEmpty }
    let sink = Sink(callback: callback, ctx: ctx)
    let generation = BridgeState.shared.start(sink: sink, productIds: ids)

    guard #available(iOS 15.0, macOS 12.0, tvOS 15.0, watchOS 8.0, *) else {
        // Below the StoreKit 2 floor. The app's deployment target should make
        // this unreachable; report it rather than pretending to be connecting.
        // `blocked`, not `unavailable`: the OS version cannot change while the
        // app runs, so there is nothing here for a retry to reach.
        sink.send(.phase, PhaseCode.blocked.rawValue, 0, "StoreKit 2 requires iOS 15 / macOS 12")
        return
    }

    sink.send(.phase, PhaseCode.connecting.rawValue)

    if let generation {
        // Long-running: transactions that arrive without a purchase call —
        // Ask to Buy approvals, purchases made on another device, subscription
        // renewals, and anything the App Store retried after a crash. Apple
        // requires this listener to exist for the app's whole lifetime.
        Task.detached(priority: .background) {
            for await update in Transaction.updates {
                if case .verified(let transaction) = update {
                    await transaction.finish()
                }
                await refreshSnapshot()
            }
            BridgeState.shared.listenerEnded(generation)
        }
    }

    Task.detached(priority: .userInitiated) {
        await refreshSnapshot()
    }
}

@_cdecl("cranpose_storekit_is_connected")
public func cranpose_storekit_is_connected() -> Bool {
    BridgeState.shared.listenerActive
}

/// Begin a purchase for `productId`. Presents the App Store payment sheet.
@_cdecl("cranpose_storekit_purchase")
public func cranpose_storekit_purchase(_ productId: UnsafePointer<CChar>) {
    let id = String(cString: productId)
    guard let sink = BridgeState.shared.sink else { return }
    guard #available(iOS 15.0, macOS 12.0, tvOS 15.0, watchOS 8.0, *) else {
        sink.send(.event, EventCode.failed.rawValue, 0, "The App Store is unavailable on this system")
        return
    }
    sink.send(.busy, 1)
    Task.detached(priority: .userInitiated) {
        await runPurchase(id: id, sink: sink)
        sink.send(.busy, 0)
        await refreshSnapshot()
    }
}

/// Re-query what this Apple ID owns, after an explicit "Restore purchases".
///
/// `AppStore.sync()` can prompt for the App Store password, so Apple's rule is
/// that it runs only from a user action — never at launch. Launch restoration
/// is what `Transaction.currentEntitlements` already does for free.
@_cdecl("cranpose_storekit_restore")
public func cranpose_storekit_restore() {
    guard let sink = BridgeState.shared.sink else { return }
    guard #available(iOS 15.0, macOS 12.0, tvOS 15.0, watchOS 8.0, *) else {
        sink.send(.event, EventCode.failed.rawValue, 0, "The App Store is unavailable on this system")
        return
    }
    sink.send(.busy, 1)
    Task.detached(priority: .userInitiated) {
        do {
            try await AppStore.sync()
        } catch {
            // A cancelled password prompt lands here too. Report it as a
            // finished restore that found whatever is already known, rather
            // than as a failure — the snapshot below is still authoritative.
            sink.send(.event, EventCode.failed.rawValue, 0, error.localizedDescription)
        }
        let owned = await ownedProductIds()
        await refreshSnapshot()
        sink.send(.event, EventCode.restored.rawValue, Int32(owned.count))
        sink.send(.busy, 0)
    }
}

// MARK: - StoreKit work

@available(iOS 15.0, macOS 12.0, tvOS 15.0, watchOS 8.0, *)
private func runPurchase(id: String, sink: Sink) async {
    do {
        let products = try await Product.products(for: [id])
        guard let product = products.first else {
            sink.send(.event, EventCode.failed.rawValue, 0, "That product is not available in your region’s App Store")
            return
        }
        let result = try await product.purchase()
        switch result {
        case .success(let verification):
            switch verification {
            case .verified(let transaction):
                // Verified means Apple's signature over the transaction
                // checked out against the device's copy of the certificate
                // chain — this is the on-device receipt validation, and it is
                // done before we grant anything.
                await transaction.finish()
                sink.send(.event, EventCode.purchased.rawValue, 0, transaction.productID)
            case .unverified(_, let error):
                // A transaction that fails signature verification is never
                // granted; a jailbreak or a proxy is the usual cause.
                sink.send(.event, EventCode.failed.rawValue, 0,
                          "This purchase could not be verified: \(error.localizedDescription)")
            }
        case .userCancelled:
            sink.send(.event, EventCode.cancelled.rawValue)
        case .pending:
            // Ask to Buy, or a payment method needing action. It may complete
            // later; `Transaction.updates` is what will tell us.
            sink.send(.event, EventCode.pending.rawValue)
        @unknown default:
            sink.send(.event, EventCode.failed.rawValue, 0, "The App Store returned an unexpected result")
        }
    } catch {
        sink.send(.event, EventCode.failed.rawValue, 0, describe(error))
    }
}

/// Turn a StoreKit error into something that names the actual cause.
///
/// `localizedDescription` on these is close to useless while integrating —
/// `Product.PurchaseError.productUnavailable` renders as "Item Not Available",
/// which reads like a store outage when it almost always means the product is
/// not in a purchasable *state* yet. The distinction costs hours if the
/// message does not make it, so the typed cases are unwrapped here.
@available(iOS 15.0, macOS 12.0, tvOS 15.0, watchOS 8.0, *)
private func describe(_ error: Error) -> String {
    if let purchaseError = error as? Product.PurchaseError {
        switch purchaseError {
        case .productUnavailable:
            return "The App Store will not sell this product yet. In App Store "
                + "Connect the in-app purchase has to reach “Ready to Submit”: "
                + "availability, price, localization and a review screenshot "
                + "must all be filled in. A product with missing metadata still "
                + "returns a price, which is why this fails only at purchase."
        case .purchaseNotAllowed:
            return "Purchases are not allowed on this device — check Screen Time → Content & Privacy Restrictions."
        case .ineligibleForOffer:
            return "This account is not eligible for that offer."
        case .invalidOfferIdentifier, .invalidOfferPrice, .invalidOfferSignature, .missingOfferParameters:
            return "The promotional offer attached to this purchase is not valid."
        case .invalidQuantity:
            return "That quantity cannot be bought."
        @unknown default:
            return "The purchase could not be completed (\(purchaseError))."
        }
    }
    if let storeError = error as? StoreKitError {
        switch storeError {
        case .userCancelled:
            return "Purchase cancelled."
        case .notAvailableInStorefront:
            return "This product is not available in your App Store country. "
                + "Check the in-app purchase's Availability, and which storefront "
                + "the signed-in sandbox account belongs to."
        case .notEntitled:
            return "This app is not entitled to make purchases — the App ID needs the In-App Purchase capability."
        case .networkError(let underlying):
            return "The App Store is unreachable: \(underlying.localizedDescription)"
        case .systemError(let underlying):
            return "The App Store returned a system error: \(underlying.localizedDescription)"
        case .unknown:
            return "The App Store returned an unknown error. On a sandbox account this usually means the account has not finished signing in — Settings → Developer → Sandbox Apple Account."
        @unknown default:
            return "The App Store returned an error (\(storeError))."
        }
    }
    return error.localizedDescription
}

/// Product ids this Apple ID currently owns, signature-verified, each with the
/// id of the transaction that granted it.
///
/// The transaction id is the paper trail an app needs to quote if an
/// entitlement is ever in dispute; ownership itself is the product id being
/// present. StoreKit always has one here, unlike Play, but a caller must still
/// treat it as optional -- the Android backend cannot always supply it.
@available(iOS 15.0, macOS 12.0, tvOS 15.0, watchOS 8.0, *)
private func ownedProductIds() async -> [(id: String, orderId: String)] {
    var owned: [(id: String, orderId: String)] = []
    for await entitlement in Transaction.currentEntitlements {
        guard case .verified(let transaction) = entitlement else { continue }
        if transaction.revocationDate != nil { continue }
        if let expires = transaction.expirationDate, expires < Date() { continue }
        owned.append((transaction.productID, String(transaction.id)))
    }
    return owned
}

/// Fetch products + entitlements and push one committed snapshot.
@available(iOS 15.0, macOS 12.0, tvOS 15.0, watchOS 8.0, *)
private func refreshSnapshot() async {
    guard let sink = BridgeState.shared.sink else { return }
    let ids = BridgeState.shared.productIds

    var products: [Product] = []
    var failure: String?
    if !ids.isEmpty {
        do {
            products = try await Product.products(for: Set(ids))
        } catch {
            failure = error.localizedDescription
        }
    }
    let owned = await ownedProductIds()

    sink.send(.begin)
    for product in products {
        sink.send(
            .product, 0, 0,
            product.id,
            product.displayPrice,
            product.displayName,
            product.description
        )
    }
    for entitlement in owned {
        sink.send(.owned, 0, 0, entitlement.id, entitlement.orderId)
    }
    // Entitlements are authoritative even when the product lookup failed, so
    // a store outage never revokes an unlock the user already paid for: the
    // phase still commits as ready, with the error attached for diagnostics.
    let ready = failure == nil || !owned.isEmpty
    sink.send(
        .phase,
        (ready ? PhaseCode.ready : PhaseCode.connecting).rawValue,
        0,
        failure
    )
}
