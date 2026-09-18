import { NavLink } from "react-router";
import { navItems } from "../../components/navigation/navItems";

export function NavigationLinks({
  items = navItems,
  detailed = false,
  onNavigate,
  onPlayRequest,
  controllerDockOpen = false,
  onControllerToggle,
}: {
  items?: typeof navItems;
  detailed?: boolean;
  onNavigate?: () => void;
  onPlayRequest?: () => void;
  controllerDockOpen?: boolean;
  onControllerToggle?: () => void;
}) {
  return (
    <nav className="primary-nav" aria-label="RackForge sections">
      {items.map((item) => {
        const Icon = item.icon;
        if (item.path === "/controller" && onControllerToggle) {
          return (
            <button
              key={item.path}
              type="button"
              onClick={() => {
                onControllerToggle();
                onNavigate?.();
              }}
              className={`nav-item nav-tint-${item.tint} controller-toggle${
                controllerDockOpen ? " dock-open" : ""
              }`}
              aria-pressed={controllerDockOpen}
            >
              <span className="nav-mark">
                <Icon aria-hidden="true" strokeWidth={1.9} />
              </span>
              <span className="nav-copy">
                <span>{item.label}</span>
                {detailed ? <small>{item.detail}</small> : null}
              </span>
            </button>
          );
        }
        return (
          <NavLink
            key={item.path}
            to={item.path}
            end={false}
            onClick={(event) => {
              if (item.path === "/play" && onPlayRequest) {
                event.preventDefault();
                onNavigate?.();
                onPlayRequest();
                return;
              }
              onNavigate?.();
            }}
            className={({ isActive }) =>
              `nav-item nav-tint-${item.tint}${isActive ? " active" : ""}`
            }
          >
            <span className="nav-mark">
              <Icon aria-hidden="true" strokeWidth={1.9} />
            </span>
            <span className="nav-copy">
              <span>{item.label}</span>
              {detailed ? <small>{item.detail}</small> : null}
            </span>
          </NavLink>
        );
      })}
    </nav>
  );
}
