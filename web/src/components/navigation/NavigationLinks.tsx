import { ChevronDown, ChevronUp } from "lucide-react";
import { useEffect, useRef, useState, type ReactNode } from "react";
import { NavLink } from "react-router";
import { navItems } from "../../components/navigation/navItems";

export function NavigationLinks({
  items = navItems,
  detailed = false,
  onNavigate,
  onPlayRequest,
  controllerDockOpen = false,
  onControllerToggle,
  scrollCues = false,
}: {
  items?: typeof navItems;
  detailed?: boolean;
  onNavigate?: () => void;
  onPlayRequest?: () => void;
  controllerDockOpen?: boolean;
  onControllerToggle?: () => void;
  /** The rail's list, which scrolls on a short window: say so. */
  scrollCues?: boolean;
}) {
  const links = items.map((item) => {
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
  });
  if (!scrollCues) {
    return (
      <nav className="primary-nav" aria-label="RackForge sections">
        {links}
      </nav>
    );
  }
  return <ScrollingNavigation>{links}</ScrollingNavigation>;
}

/**
 * The rail's sections in a list that scrolls when the window is too short
 * for them -- a handheld at a tablet's interface size -- and shows that it
 * does: the list fades out at an edge with more beyond it, and a chevron
 * there moves it on. Neither shows while everything fits.
 */
function ScrollingNavigation({ children }: { children: ReactNode }) {
  const navRef = useRef<HTMLElement>(null);
  const [more, setMore] = useState({ above: false, below: false });

  useEffect(() => {
    const nav = navRef.current;
    if (!nav) return;
    const measure = () => {
      const above = nav.scrollTop > 1;
      const below = nav.scrollTop + nav.clientHeight < nav.scrollHeight - 1;
      setMore((previous) =>
        previous.above === above && previous.below === below ? previous : { above, below });
    };
    measure();
    nav.addEventListener("scroll", measure, { passive: true });
    const observer = new ResizeObserver(measure);
    observer.observe(nav);
    for (const child of Array.from(nav.children)) observer.observe(child);
    return () => {
      nav.removeEventListener("scroll", measure);
      observer.disconnect();
    };
  }, []);

  const page = (direction: 1 | -1) => {
    const nav = navRef.current;
    if (!nav) return;
    nav.scrollBy({ top: direction * nav.clientHeight * 0.7, behavior: "smooth" });
  };

  return (
    <div
      className="primary-nav-frame"
      data-more-above={more.above ? "" : undefined}
      data-more-below={more.below ? "" : undefined}
    >
      <nav ref={navRef} className="primary-nav" aria-label="RackForge sections">
        {children}
      </nav>
      {/* Pointer and touch only: from the keyboard, each section brings
          itself into view as it takes the focus. */}
      <button
        type="button"
        className="primary-nav-cue primary-nav-cue-up"
        tabIndex={-1}
        aria-hidden="true"
        onClick={() => page(-1)}
      >
        <ChevronUp strokeWidth={2.2} />
      </button>
      <button
        type="button"
        className="primary-nav-cue primary-nav-cue-down"
        tabIndex={-1}
        aria-hidden="true"
        onClick={() => page(1)}
      >
        <ChevronDown strokeWidth={2.2} />
      </button>
    </div>
  );
}
