use super::*;

#[test]
fn each_answer_reaches_only_the_service_that_asked() {
    let results = "android.permission.CAMERA\t1\nandroid.permission.RECORD_AUDIO\t0\n";
    assert_eq!(answer(results, &HEART_RATE_PERMISSIONS), None);
    assert_eq!(answer(results, &[MICROPHONE_PERMISSION]), Some(false));
    assert_eq!(
        answer(
            "android.permission.health.READ_HEART_RATE\t1\n",
            &HEART_RATE_PERMISSIONS
        ),
        Some(true)
    );
    assert_eq!(
        answer(
            "android.permission.BODY_SENSORS\t0\n",
            &HEART_RATE_PERMISSIONS
        ),
        Some(false)
    );
}
