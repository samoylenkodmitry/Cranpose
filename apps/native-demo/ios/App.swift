import UIKit
import Cranpose
import CranposeBindings

@main
final class AppDelegate: UIResponder, UIApplicationDelegate {
    var window: UIWindow?
    func application(_ application: UIApplication,
                     didFinishLaunchingWithOptions options: [UIApplication.LaunchOptionsKey: Any]?) -> Bool {
        let window = UIWindow(frame: UIScreen.main.bounds)
        window.rootViewController = DemoController()
        window.makeKeyAndVisible()
        self.window = window
        return true
    }
}

final class DemoController: UIViewController {
    private let stack = UIStackView()
    private let count = UILabel()
    private var component: CranposeView?
    private let modes = UISegmentedControl(items: ["Native → Cranpose", "Cranpose → Native"])

    override func viewDidLoad() {
        super.viewDidLoad()
        view.backgroundColor = .systemBackground
        stack.axis = .vertical
        stack.spacing = 14
        stack.translatesAutoresizingMaskIntoConstraints = false
        view.addSubview(stack)
        NSLayoutConstraint.activate([
            stack.topAnchor.constraint(equalTo: view.safeAreaLayoutGuide.topAnchor, constant: 12),
            stack.leadingAnchor.constraint(equalTo: view.leadingAnchor, constant: 16),
            stack.trailingAnchor.constraint(equalTo: view.trailingAnchor, constant: -16),
            stack.bottomAnchor.constraint(lessThanOrEqualTo: view.safeAreaLayoutGuide.bottomAnchor, constant: -12)
        ])
        let title = UILabel()
        title.text = "Cranpose + UIKit"
        title.font = .preferredFont(forTextStyle: .largeTitle)
        stack.addArrangedSubview(title)
        modes.selectedSegmentIndex = ProcessInfo.processInfo.arguments.contains("--native-component") ? 0 : 1
        modes.addTarget(self, action: #selector(changeMode), for: .valueChanged)
        stack.addArrangedSubview(modes)
        let button = UIButton(type: .system)
        button.setTitle("Add from native UIKit", for: .normal)
        button.accessibilityIdentifier = "native-increment"
        button.addTarget(self, action: #selector(increment), for: .touchUpInside)
        stack.addArrangedSubview(button)
        count.text = "Host received: 0"
        count.numberOfLines = 0
        count.accessibilityIdentifier = "host-count"
        stack.addArrangedSubview(count)
        changeMode()
    }

    @objc private func increment() { component?.sendEvent("increment") }

    @objc private func changeMode() {
        component?.close()
        component?.removeFromSuperview()
        let child = CranposeView(session: createDemoComponent(nativeChildren: modes.selectedSegmentIndex == 1, website: "https://example.com"),
            factories: ["web": WebViewFactory()])
        child.onEvent = { [weak self] event in
            if event.name == "count" { self?.count.text = "Host received: \(event.value)" }
        }
        child.onError = { [weak self] error in
            print("Cranpose native host: \(error)")
            self?.count.text = error
        }
        stack.addArrangedSubview(child)
        let height = child.heightAnchor.constraint(equalToConstant: modes.selectedSegmentIndex == 0 ? 260 : 560)
        height.priority = .defaultHigh
        height.isActive = true
        component = child
    }
}
