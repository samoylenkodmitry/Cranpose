package dev.cranpose.gradle

import org.gradle.api.DefaultTask
import org.gradle.api.GradleException
import org.gradle.api.file.RegularFileProperty
import org.gradle.api.provider.MapProperty
import org.gradle.api.provider.SetProperty
import org.gradle.api.tasks.Input
import org.gradle.api.tasks.InputFile
import org.gradle.api.tasks.OutputFile
import org.gradle.api.tasks.TaskAction
import org.w3c.dom.Element
import javax.xml.parsers.DocumentBuilderFactory
import javax.xml.transform.OutputKeys
import javax.xml.transform.TransformerFactory
import javax.xml.transform.dom.DOMSource
import javax.xml.transform.stream.StreamResult

private const val ANDROID_NAMESPACE = "http://schemas.android.com/apk/res/android"

/**
 * The hardware features Android's packaging tools demand on behalf of a
 * permission, unless the manifest names the feature itself.
 *
 * A permission here costs the application every device without that hardware:
 * `android.permission.CAMERA` alone takes it off every machine with no camera,
 * Play reports the loss only when the release is already prepared, and users
 * on those devices stop getting updates. Naming the feature with
 * `android:required="false"` is what stops it, which is what
 * [CranposeManifestCheck] writes for each one.
 */
internal val FEATURES_BEHIND_PERMISSIONS: Map<String, List<String>> = mapOf(
    "android.permission.CAMERA" to listOf("android.hardware.camera"),
    "android.permission.RECORD_AUDIO" to listOf("android.hardware.microphone"),
    "android.permission.NFC" to listOf("android.hardware.nfc"),
    "android.permission.BLUETOOTH" to listOf("android.hardware.bluetooth"),
    "android.permission.BLUETOOTH_ADMIN" to listOf("android.hardware.bluetooth"),
    "android.permission.ACCESS_FINE_LOCATION" to
        listOf("android.hardware.location.gps", "android.hardware.location"),
    "android.permission.ACCESS_COARSE_LOCATION" to
        listOf("android.hardware.location.network", "android.hardware.location"),
    "android.permission.ACCESS_LOCATION_EXTRA_COMMANDS" to listOf("android.hardware.location"),
    "android.permission.INSTALL_LOCATION_PROVIDER" to listOf("android.hardware.location"),
    "android.permission.ACCESS_WIFI_STATE" to listOf("android.hardware.wifi"),
    "android.permission.CHANGE_WIFI_STATE" to listOf("android.hardware.wifi"),
    "android.permission.CHANGE_WIFI_MULTICAST_STATE" to listOf("android.hardware.wifi"),
    "android.permission.CALL_PHONE" to listOf("android.hardware.telephony"),
    "android.permission.CALL_PRIVILEGED" to listOf("android.hardware.telephony"),
    "android.permission.MODIFY_PHONE_STATE" to listOf("android.hardware.telephony"),
    "android.permission.PROCESS_OUTGOING_CALLS" to listOf("android.hardware.telephony"),
    "android.permission.READ_SMS" to listOf("android.hardware.telephony"),
    "android.permission.RECEIVE_SMS" to listOf("android.hardware.telephony"),
    "android.permission.RECEIVE_MMS" to listOf("android.hardware.telephony"),
    "android.permission.RECEIVE_WAP_PUSH" to listOf("android.hardware.telephony"),
    "android.permission.SEND_SMS" to listOf("android.hardware.telephony"),
    "android.permission.WRITE_APN_SETTINGS" to listOf("android.hardware.telephony"),
    "android.permission.WRITE_SMS" to listOf("android.hardware.telephony"),
)

/**
 * One feature declaration as the merged manifest holds it.
 */
internal data class FeatureLine(val name: String, val required: Boolean)

/**
 * What the merged manifest says about features, and what should be done to it.
 *
 * [missing] are the features a permission demands that nothing declares yet;
 * each is written as `required="false"` unless the application asked for it.
 * [unwanted] are the features that stay required although the application
 * never named them, which is the regression this task refuses to ship.
 */
internal data class FeaturePlan(
    val missing: List<FeatureLine>,
    val unwanted: List<String>,
    val reasons: Map<String, String>,
)

/**
 * Works out which feature declarations the merged manifest needs.
 *
 * Kept apart from the file reads and writes so the rules can be read, and
 * tested, on their own.
 */
internal fun planFeatures(
    permissions: List<String>,
    declared: List<FeatureLine>,
    wanted: Set<String>,
): FeaturePlan {
    val reasons = mutableMapOf<String, String>()
    for (permission in permissions) {
        for (feature in FEATURES_BEHIND_PERMISSIONS[permission].orEmpty()) {
            reasons.putIfAbsent(feature, permission)
        }
    }

    val names = declared.map(FeatureLine::name).toSet()
    val missing = reasons.keys
        .filterNot(names::contains)
        .sorted()
        .map { name -> FeatureLine(name, wanted.contains(name)) }

    val unwanted = declared
        .filter { line -> line.required && !wanted.contains(line.name) }
        .map(FeatureLine::name)
        .sorted()

    return FeaturePlan(missing = missing, unwanted = unwanted, reasons = reasons)
}

/**
 * The message a build fails with when a service's permission is not declared.
 *
 * [missing] maps each permission to the service that needs it.
 */
internal fun missingPermissionText(missing: Map<String, String>): String {
    val named = missing.keys.sorted()
    val lines = named.joinToString("\n") { permission ->
        "  $permission (the ${missing.getValue(permission)} service)"
    }
    val manifest = named.joinToString("\n") { permission ->
        "  <uses-permission android:name=\"$permission\" />"
    }
    return "Services this application uses need permissions it does not declare:\n" +
        lines +
        "\n\nCranpose declares no permission of its own: a permission is a line in the " +
        "store listing and a question to the person holding the phone, so it belongs " +
        "in this application's own AndroidManifest.xml.\n\n" +
        manifest +
        "\n\nOr drop the service from cranpose { services } if the application does not " +
        "use it."
}

/**
 * The message a build fails with when a feature nobody asked for is required.
 */
internal fun refusalText(unwanted: List<String>, reasons: Map<String, String>): String {
    val lines = unwanted.joinToString("\n") { name ->
        val permission = reasons[name]
        if (permission == null) {
            "  $name (declared android:required=\"true\" in a manifest)"
        } else {
            "  $name (comes with $permission)"
        }
    }
    return "This build requires hardware features the application never asked for:\n" +
        lines +
        "\n\nEvery such feature takes the application off every device without that " +
        "hardware, and off Play for users who already have it installed. Name the ones " +
        "that are genuinely needed:\n\n" +
        "cranpose {\n" +
        unwanted.joinToString("\n") { name -> "    requiredFeatures.add(\"$name\")" } +
        "\n}\n\n" +
        "or declare them android:required=\"false\" in the manifest that asks for them."
}

/**
 * Refuses a build whose manifest does not say what the application does.
 *
 * Two rules. A service the application uses needs its permissions declared in
 * the application's own manifest, because Cranpose declares none. A hardware
 * feature a permission carries stays optional unless
 * `cranpose { requiredFeatures }` names it, and the missing declarations are
 * written here.
 *
 * The task transforms the merged manifest, so it sees everything: the
 * framework's contributions, the application's own manifest, and any library's.
 */
abstract class CranposeManifestCheck : DefaultTask() {

    /** The merged manifest AGP produced. */
    @get:InputFile
    abstract val mergedManifest: RegularFileProperty

    /** The merged manifest with the feature declarations added. */
    @get:OutputFile
    abstract val updatedManifest: RegularFileProperty

    /** The features named by `cranpose { requiredFeatures }`. */
    @get:Input
    abstract val requiredFeatures: SetProperty<String>

    /** Each permission the application's services need, and the service that needs it. */
    @get:Input
    abstract val servicePermissions: MapProperty<String, String>

    @TaskAction
    fun run() {
        val builder = DocumentBuilderFactory.newInstance()
            .apply { isNamespaceAware = true }
            .newDocumentBuilder()
        val document = builder.parse(mergedManifest.get().asFile)
        val manifest = document.documentElement

        val permissions = document.getElementsByTagName("uses-permission")
            .let { nodes -> (0 until nodes.length).mapNotNull { at -> nodes.item(at) as? Element } }
            .mapNotNull { element -> element.getAttributeNS(ANDROID_NAMESPACE, "name").takeIf(String::isNotEmpty) }

        val featureElements = document.getElementsByTagName("uses-feature")
            .let { nodes -> (0 until nodes.length).mapNotNull { at -> nodes.item(at) as? Element } }
        val declared = featureElements.mapNotNull { element ->
            val name = element.getAttributeNS(ANDROID_NAMESPACE, "name")
            if (name.isEmpty()) {
                null
            } else {
                val required = element.getAttributeNS(ANDROID_NAMESPACE, "required")
                FeatureLine(name, required.isEmpty() || required.toBoolean())
            }
        }

        val missing = servicePermissions.get().filterKeys { name -> !permissions.contains(name) }
        if (missing.isNotEmpty()) {
            throw GradleException(missingPermissionText(missing))
        }

        val plan = planFeatures(permissions, declared, requiredFeatures.get())
        if (plan.unwanted.isNotEmpty()) {
            throw GradleException(refusalText(plan.unwanted, plan.reasons))
        }

        for (line in plan.missing) {
            val element = document.createElement("uses-feature")
            element.setAttributeNS(ANDROID_NAMESPACE, "android:name", line.name)
            element.setAttributeNS(ANDROID_NAMESPACE, "android:required", line.required.toString())
            manifest.appendChild(element)
            logger.lifecycle(
                "cranpose: ${line.name} declared android:required=\"${line.required}\" " +
                    "for ${plan.reasons[line.name]}"
            )
        }

        val writer = TransformerFactory.newInstance().newTransformer()
        writer.setOutputProperty(OutputKeys.INDENT, "yes")
        writer.transform(DOMSource(document), StreamResult(updatedManifest.get().asFile))
    }
}
