package dev.cranpose.android;

import android.app.Activity;
import android.content.Context;
import android.os.Handler;
import android.os.Looper;
import android.util.Log;

import com.android.billingclient.api.AcknowledgePurchaseParams;
import com.android.billingclient.api.BillingClient;
import com.android.billingclient.api.BillingClientStateListener;
import com.android.billingclient.api.BillingFlowParams;
import com.android.billingclient.api.BillingResult;
import com.android.billingclient.api.PendingPurchasesParams;
import com.android.billingclient.api.ProductDetails;
import com.android.billingclient.api.Purchase;
import com.android.billingclient.api.PurchasesUpdatedListener;
import com.android.billingclient.api.QueryProductDetailsParams;
import com.android.billingclient.api.QueryPurchasesParams;

import java.util.ArrayList;
import java.util.Collections;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.TreeSet;

/**
 * Google Play Billing behind {@code cranpose_services::purchases}.
 *
 * <p>This class lives in its own source directory, {@code
 * crates/cranpose/android/java-billing}, rather than next to {@link CranposeActivity}: it is
 * the only part of Cranpose that needs {@code com.android.billingclient}, and an app that
 * sells nothing should not have to declare that dependency. An app that does sell something
 * adds the directory and the dependency together, and builds the Rust side with the
 * {@code playbilling} feature. No change to the app's activity is needed — the Rust backend
 * calls the static methods below and passes the activity in, because the payment sheet is an
 * activity result.
 *
 * <p>Threading. The {@code cranpose*} methods are called from the native-activity thread and
 * return immediately; every billing call they make is asynchronous, and
 * {@code launchBillingFlow} is dispatched to the Java UI thread, which is the only thread the
 * platform accepts it on. The {@code native*} methods run on whichever thread the billing
 * library answers on and only hand data across; the Rust side parks it and wakes the frame
 * loop.
 *
 * <p>What crosses JNI. State goes over as one flattened snapshot per update rather than one
 * call per product, in the format {@code crates/cranpose/src/android_purchase_wire.rs}
 * decodes. Because this class holds everything the billing client has told it, each snapshot
 * is complete: the Rust side can swap it in wholesale and never shows a half-built price
 * list.
 */
public final class CranposeBilling implements PurchasesUpdatedListener {

    private static final String TAG = "cranpose";

    /** Mirrors {@code StorePhase} in {@code cranpose_services::purchases}. */
    private static final int PHASE_UNAVAILABLE = 0;
    private static final int PHASE_CONNECTING = 1;
    private static final int PHASE_READY = 2;
    private static final int PHASE_BLOCKED = 3;

    /** Mirrors the event codes in {@code android_purchase_wire.rs}. */
    private static final int EVENT_PURCHASED = 0;
    private static final int EVENT_CANCELLED = 1;
    private static final int EVENT_PENDING = 2;
    private static final int EVENT_FAILED = 3;
    private static final int EVENT_RESTORED = 4;

    /**
     * The one client for the process. A {@code BillingClient} is expensive to build and
     * holds a bound service, and the Rust side has no object to hang it off: it reaches this
     * class through the activity's class loader, by name.
     */
    private static volatile CranposeBilling instance;

    private final BillingClient client;

    /** Guards everything the snapshot is built from; the billing library answers on its own threads. */
    private final Object lock = new Object();

    private final List<String> productIds = new ArrayList<>();
    private final Map<String, ProductDetails> products = new LinkedHashMap<>();
    private final Set<String> owned = new TreeSet<>();
    private boolean productsKnown;
    private boolean ownedKnown;
    private boolean connected;
    private boolean connecting;

    /**
     * Set when setup failed with a response Play will keep giving. In-app billing turned off
     * on the device, an account that cannot pay, a country the app is not sold in: the
     * client reconnects on its own, and every attempt is answered the same way. Reporting
     * that as a store we have merely not reached yet leaves an app inviting a retry that
     * cannot work, so it becomes its own phase and the app can say why instead.
     */
    private boolean blocked;
    private boolean busy;
    private boolean pendingRestore;
    private String error = "";

    /**
     * Acknowledgement attempts still owed, by purchase token. Play revokes and refunds a
     * purchase that is not acknowledged within three days, so a failed acknowledgement is a
     * bug that costs the buyer their content and the developer the sale — it cannot be a log
     * line. The count bounds the backoff; the map also stops two routes to the same purchase
     * from acknowledging it twice at once.
     */
    private final Map<String, Integer> acknowledgeAttempts = new LinkedHashMap<>();

    /**
     * Retries are posted here rather than slept for: every billing callback arrives on a
     * library thread that must be given back.
     */
    private final Handler retries = new Handler(Looper.getMainLooper());

    private CranposeBilling(Context context) {
        client =
                BillingClient.newBuilder(context.getApplicationContext())
                        .setListener(this)
                        // Play can drop the binding at any time (an update, low memory). Without
                        // this every later call would fail until something reconnected by hand.
                        .enableAutoServiceReconnection()
                        // Required since Play Billing 6: one-time products can complete after the
                        // flow closes, which is the PENDING state the API reports as an event.
                        .enablePendingPurchases(
                                PendingPurchasesParams.newBuilder().enableOneTimeProducts().build())
                        .build();
    }

    // ------------------------------------------------------------------
    // Called from Rust (native-activity thread) through the activity's
    // class loader. Each one returns as soon as it has handed the work to
    // the billing library.
    // ------------------------------------------------------------------

    /**
     * Declares the products this app sells and connects to the store.
     *
     * <p>Idempotent: called again on relaunch, and again whenever the app reopens its store
     * screen. Product ids arrive newline separated, which is unambiguous because a Play
     * product id is lower-case alphanumerics, dots and underscores.
     */
    public static void cranposeBillingConfigure(Activity activity, String productIds) {
        CranposeBilling billing = attach(activity);
        if (billing == null) {
            return;
        }
        billing.configure(productIds);
    }

    /** Presents the Play payment sheet for one product. */
    public static void cranposeBillingPurchase(Activity activity, String productId) {
        CranposeBilling billing = attach(activity);
        if (billing == null) {
            return;
        }
        billing.purchase(activity, productId);
    }

    /** Re-queries what the account owns, for an explicit "restore purchases" button. */
    public static void cranposeBillingRestore(Activity activity) {
        CranposeBilling billing = attach(activity);
        if (billing == null) {
            return;
        }
        billing.restore();
    }

    /**
     * The process-wide client, built on first use.
     *
     * <p>Returns null only when the billing library is not on the class path, which is the
     * one failure an app can hit by enabling the Rust feature without the Gradle dependency.
     * It is reported as "no store" rather than thrown, so the app keeps running with
     * everything it sells simply unavailable.
     */
    private static CranposeBilling attach(Activity activity) {
        CranposeBilling billing = instance;
        if (billing != null) {
            return billing;
        }
        synchronized (CranposeBilling.class) {
            if (instance == null) {
                try {
                    instance = new CranposeBilling(activity);
                } catch (Throwable failure) {
                    Log.w(TAG, "Play Billing is not available", failure);
                    nativeBillingSnapshot(
                            PHASE_UNAVAILABLE + "\t0\t" + escape(String.valueOf(failure)));
                    return null;
                }
            }
            return instance;
        }
    }

    // ------------------------------------------------------------------
    // Rust callbacks. Declared here and resolved against the app's native
    // library, which CranposeActivity has already loaded by the time any
    // of this runs.
    // ------------------------------------------------------------------

    /** The whole store snapshot, in the format android_purchase_wire.rs decodes. */
    private static native void nativeBillingSnapshot(String payload);

    /** A one-shot event a snapshot cannot express (cancelled, pending, failed, restored). */
    private static native void nativeBillingEvent(int code, String message, int count);

    // ------------------------------------------------------------------
    // Store conversation
    // ------------------------------------------------------------------

    private void configure(String ids) {
        synchronized (lock) {
            productIds.clear();
            for (String id : ids.split("\n")) {
                if (!id.isEmpty()) {
                    productIds.add(id);
                }
            }
        }
        connect();
    }

    /**
     * Re-queries what the account owns and announces the result.
     *
     * <p>When the client is not up yet the announcement is handed to the connection attempt,
     * so one press produces one answer instead of failing against a client that is still
     * coming up.
     */
    private void restore() {
        setBusy(true);
        if (client.isReady()) {
            queryPurchases(true);
            return;
        }
        synchronized (lock) {
            pendingRestore = true;
        }
        connect();
    }

    private boolean takePendingRestore() {
        synchronized (lock) {
            boolean pending = pendingRestore;
            pendingRestore = false;
            return pending;
        }
    }

    private void connect() {
        try {
            switch (client.getConnectionState()) {
                case BillingClient.ConnectionState.CONNECTED:
                    queryProducts();
                    queryPurchases(takePendingRestore());
                    return;
                case BillingClient.ConnectionState.CONNECTING:
                    // A second start would be answered with DEVELOPER_ERROR; the listener
                    // installed by the first one still reports for this caller.
                    return;
                default:
                    break;
            }
            synchronized (lock) {
                connecting = true;
                // A fresh attempt has not been refused yet, and the app should see
                // "connecting" rather than the verdict of the last one.
                blocked = false;
            }
            pushSnapshot();
            client.startConnection(
                    new BillingClientStateListener() {
                        @Override
                        public void onBillingSetupFinished(BillingResult result) {
                            if (result.getResponseCode() == BillingClient.BillingResponseCode.OK) {
                                synchronized (lock) {
                                    connected = true;
                                    connecting = false;
                                    blocked = false;
                                    error = "";
                                }
                                queryProducts();
                                queryPurchases(takePendingRestore());
                                return;
                            }
                            // The store said no. Reporting this as "still connecting" would
                            // leave the app waiting forever for prices that are not coming.
                            synchronized (lock) {
                                connected = false;
                                connecting = false;
                                blocked = !isRetryable(result.getResponseCode());
                            }
                            if (takePendingRestore()) {
                                nativeBillingEvent(EVENT_FAILED, userMessage(result), 0);
                            }
                            setBusy(false);
                            fail("billing setup", result);
                        }

                        @Override
                        public void onBillingServiceDisconnected() {
                            // Auto-reconnection is on, so this is a gap rather than an
                            // absence, and what the store already told us stays true.
                            synchronized (lock) {
                                connected = false;
                                connecting = true;
                            }
                            pushSnapshot();
                        }
                    });
        } catch (Throwable failure) {
            Log.w(TAG, "billing connect failed", failure);
            takePendingRestore();
            synchronized (lock) {
                connected = false;
                connecting = false;
                blocked = false;
                error = String.valueOf(failure);
            }
            setBusy(false);
            pushSnapshot();
        }
    }

    private void queryProducts() {
        List<QueryProductDetailsParams.Product> requested = new ArrayList<>();
        synchronized (lock) {
            for (String id : productIds) {
                requested.add(
                        QueryProductDetailsParams.Product.newBuilder()
                                .setProductId(id)
                                .setProductType(BillingClient.ProductType.INAPP)
                                .build());
            }
        }
        if (requested.isEmpty()) {
            return;
        }
        client.queryProductDetailsAsync(
                QueryProductDetailsParams.newBuilder().setProductList(requested).build(),
                (result, found) -> {
                    if (result.getResponseCode() != BillingClient.BillingResponseCode.OK) {
                        fail("product details", result);
                        return;
                    }
                    synchronized (lock) {
                        products.clear();
                        for (ProductDetails product : found.getProductDetailsList()) {
                            products.put(product.getProductId(), product);
                        }
                        productsKnown = true;
                        error = "";
                    }
                    pushSnapshot();
                });
    }

    private void queryPurchases(boolean announce) {
        client.queryPurchasesAsync(
                QueryPurchasesParams.newBuilder()
                        .setProductType(BillingClient.ProductType.INAPP)
                        .build(),
                (result, purchases) -> {
                    if (result.getResponseCode() != BillingClient.BillingResponseCode.OK) {
                        setBusy(false);
                        if (announce) {
                            nativeBillingEvent(EVENT_FAILED, userMessage(result), 0);
                        }
                        fail("query purchases", result);
                        return;
                    }
                    int found = apply(purchases, false);
                    setBusy(false);
                    if (announce) {
                        // Zero is worth reporting: the user pressed restore and deserves an
                        // answer either way.
                        nativeBillingEvent(EVENT_RESTORED, "", found);
                    }
                });
    }

    private void purchase(Activity activity, String productId) {
        ProductDetails product;
        synchronized (lock) {
            product = products.get(productId);
        }
        if (product == null) {
            // The price has not arrived yet, so there is nothing to charge for. Ask again
            // and let the user retry rather than opening an empty sheet.
            nativeBillingEvent(EVENT_FAILED, "The price has not loaded yet", 0);
            connect();
            return;
        }
        BillingFlowParams.ProductDetailsParams.Builder details =
                BillingFlowParams.ProductDetailsParams.newBuilder().setProductDetails(product);
        ProductDetails.OneTimePurchaseOfferDetails offer = oneTimeOffer(product);
        if (offer != null && offer.getOfferToken() != null) {
            details.setOfferToken(offer.getOfferToken());
        }
        BillingFlowParams flow =
                BillingFlowParams.newBuilder()
                        .setProductDetailsParamsList(Collections.singletonList(details.build()))
                        .build();
        setBusy(true);
        activity.runOnUiThread(
                () -> {
                    BillingResult result = client.launchBillingFlow(activity, flow);
                    if (result.getResponseCode() != BillingClient.BillingResponseCode.OK) {
                        setBusy(false);
                        nativeBillingEvent(EVENT_FAILED, userMessage(result), 0);
                        fail("purchase flow", result);
                    }
                });
    }

    @Override
    public void onPurchasesUpdated(BillingResult result, List<Purchase> purchases) {
        setBusy(false);
        switch (result.getResponseCode()) {
            case BillingClient.BillingResponseCode.OK:
                apply(purchases == null ? Collections.<Purchase>emptyList() : purchases, true);
                return;
            case BillingClient.BillingResponseCode.USER_CANCELED:
                // Not an error. The app is told so it can stop showing a spinner, and
                // nothing else.
                nativeBillingEvent(EVENT_CANCELLED, "", 0);
                return;
            case BillingClient.BillingResponseCode.ITEM_ALREADY_OWNED:
                // Bought on another device, or the entitlement outlived a reinstall.
                queryPurchases(false);
                return;
            default:
                nativeBillingEvent(EVENT_FAILED, userMessage(result), 0);
                fail("purchase", result);
        }
    }

    /**
     * Folds a list of purchases into the snapshot and returns how many entitlements it
     * carried. Acknowledgement happens here: Play refunds an unacknowledged purchase after
     * three days, so it has to follow every route the entitlement can arrive by, not just
     * the payment sheet.
     */
    private int apply(List<Purchase> purchases, boolean announce) {
        Set<String> found = new TreeSet<>();
        boolean pending = false;
        for (Purchase purchase : purchases) {
            if (purchase.getPurchaseState() == Purchase.PurchaseState.PURCHASED) {
                found.addAll(purchase.getProducts());
                acknowledge(purchase);
            } else if (purchase.getPurchaseState() == Purchase.PurchaseState.PENDING) {
                pending = true;
            }
        }
        synchronized (lock) {
            owned.clear();
            owned.addAll(found);
            ownedKnown = true;
        }
        pushSnapshot();
        if (announce) {
            for (String id : found) {
                nativeBillingEvent(EVENT_PURCHASED, id, 0);
            }
            if (found.isEmpty() && pending) {
                nativeBillingEvent(EVENT_PENDING, "", 0);
            }
        }
        return found.size();
    }

    /** How many times one purchase is acknowledged before the attempt is left to the next start. */
    private static final int MAX_ACKNOWLEDGE_ATTEMPTS = 6;
    /** First retry delay; each later one doubles it, so six attempts span about a minute. */
    private static final long ACKNOWLEDGE_RETRY_MILLIS = 1_000L;

    private void acknowledge(Purchase purchase) {
        if (purchase.isAcknowledged()) {
            return;
        }
        String token = purchase.getPurchaseToken();
        synchronized (lock) {
            if (acknowledgeAttempts.containsKey(token)) {
                // Already in flight. Every route the entitlement can arrive by lands here,
                // and a second call would ask Play to acknowledge the same purchase twice.
                return;
            }
            acknowledgeAttempts.put(token, 0);
        }
        sendAcknowledge(token);
    }

    private void sendAcknowledge(String token) {
        client.acknowledgePurchase(
                AcknowledgePurchaseParams.newBuilder().setPurchaseToken(token).build(),
                result -> {
                    if (result.getResponseCode() == BillingClient.BillingResponseCode.OK) {
                        synchronized (lock) {
                            acknowledgeAttempts.remove(token);
                        }
                        return;
                    }
                    if (!isRetryable(result.getResponseCode())) {
                        // Play will not change its mind about this one. Dropping the record
                        // lets a later start try again from scratch, which is the only thing
                        // left that can help.
                        synchronized (lock) {
                            acknowledgeAttempts.remove(token);
                        }
                        Log.w(TAG, "acknowledge refused: " + describe(result));
                        return;
                    }
                    int attempt;
                    synchronized (lock) {
                        Integer previous = acknowledgeAttempts.get(token);
                        if (previous == null) {
                            return;
                        }
                        attempt = previous + 1;
                        if (attempt >= MAX_ACKNOWLEDGE_ATTEMPTS) {
                            // Out of attempts for this run. The record is dropped so the next
                            // start, which re-queries purchases, acknowledges it again — the
                            // three-day window is far longer than one session.
                            acknowledgeAttempts.remove(token);
                            Log.w(
                                    TAG,
                                    "acknowledge gave up for this run after "
                                            + attempt
                                            + " attempts: "
                                            + describe(result));
                            return;
                        }
                        acknowledgeAttempts.put(token, attempt);
                    }
                    long delay = ACKNOWLEDGE_RETRY_MILLIS << (attempt - 1);
                    Log.w(
                            TAG,
                            "acknowledge failed ("
                                    + describe(result)
                                    + "); retrying in "
                                    + delay
                                    + " ms");
                    retries.postDelayed(() -> sendAcknowledge(token), delay);
                });
    }

    /**
     * Whether an answer from Play is worth asking again about. A network that is down or a
     * service that is busy will pass; a purchase Play does not recognise will not.
     */
    private static boolean isRetryable(int responseCode) {
        switch (responseCode) {
            case BillingClient.BillingResponseCode.SERVICE_DISCONNECTED:
            case BillingClient.BillingResponseCode.SERVICE_UNAVAILABLE:
            case BillingClient.BillingResponseCode.NETWORK_ERROR:
            case BillingClient.BillingResponseCode.ERROR:
                return true;
            default:
                return false;
        }
    }

    private static ProductDetails.OneTimePurchaseOfferDetails oneTimeOffer(ProductDetails product) {
        List<ProductDetails.OneTimePurchaseOfferDetails> offers =
                product.getOneTimePurchaseOfferDetailsList();
        if (offers == null || offers.isEmpty()) {
            return null;
        }
        return offers.get(0);
    }

    // ------------------------------------------------------------------
    // Snapshot
    // ------------------------------------------------------------------

    private void setBusy(boolean value) {
        synchronized (lock) {
            if (busy == value) {
                return;
            }
            busy = value;
        }
        pushSnapshot();
    }

    /** Records a store failure for diagnostics and republishes the snapshot. */
    private void fail(String what, BillingResult result) {
        String detail = describe(result);
        Log.w(TAG, what + " failed: " + detail);
        synchronized (lock) {
            error = detail;
        }
        pushSnapshot();
    }

    private void pushSnapshot() {
        StringBuilder payload = new StringBuilder();
        synchronized (lock) {
            int phase;
            if (connected) {
                phase = productsKnown && ownedKnown ? PHASE_READY : PHASE_CONNECTING;
            } else if (connecting) {
                phase = PHASE_CONNECTING;
            } else if (blocked) {
                phase = PHASE_BLOCKED;
            } else {
                phase = PHASE_UNAVAILABLE;
            }
            payload.append(phase).append('\t').append(busy ? '1' : '0').append('\t');
            payload.append(escape(error));
            for (ProductDetails product : products.values()) {
                ProductDetails.OneTimePurchaseOfferDetails offer = oneTimeOffer(product);
                if (offer == null) {
                    // Without an offer there is no price to show, and the API contract is
                    // that a listed product always has one.
                    continue;
                }
                payload.append('\n')
                        .append("p\t")
                        .append(escape(product.getProductId()))
                        .append('\t')
                        .append(escape(offer.getFormattedPrice()))
                        .append('\t')
                        .append(escape(product.getTitle()))
                        .append('\t')
                        .append(escape(product.getDescription()));
            }
            for (String id : owned) {
                payload.append('\n').append("o\t").append(escape(id));
            }
        }
        nativeBillingSnapshot(payload.toString());
    }

    /** The store's own words, for {@code StoreState::error} and the log. */
    private static String describe(BillingResult result) {
        return result.getResponseCode() + " " + result.getDebugMessage();
    }

    /**
     * A sentence to show the user. Play's debug messages are written for developers, so the
     * response code is translated instead of passed through.
     */
    private static String userMessage(BillingResult result) {
        switch (result.getResponseCode()) {
            case BillingClient.BillingResponseCode.BILLING_UNAVAILABLE:
            case BillingClient.BillingResponseCode.SERVICE_UNAVAILABLE:
                return "The Play Store is not available right now";
            case BillingClient.BillingResponseCode.NETWORK_ERROR:
                return "No connection to the Play Store";
            case BillingClient.BillingResponseCode.ITEM_UNAVAILABLE:
                return "This is not available on this account";
            case BillingClient.BillingResponseCode.ITEM_ALREADY_OWNED:
                return "You already own this";
            default:
                return "The purchase could not be completed";
        }
    }

    /**
     * The escaping {@code android_purchase_wire.rs} reverses. The percent sign goes first so
     * that an escape this method writes is not escaped a second time.
     */
    private static String escape(String value) {
        if (value == null) {
            return "";
        }
        return value.replace("%", "%25")
                .replace("\t", "%09")
                .replace("\n", "%0A")
                .replace("\r", "%0D");
    }
}
