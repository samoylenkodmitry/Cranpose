fn main() {
    cranpose_storekit::register();
    #[cfg(any(target_os = "ios", target_os = "macos"))]
    {
        use cranpose_services::purchases::{self, Purchases};
        purchases::configure(&["com.example.link.check"]);
        let backend = cranpose_storekit::StoreKitPurchases;
        backend.purchase("com.example.link.check");
        backend.restore();
        println!("{:?}", purchases::store_state());
    }
}
