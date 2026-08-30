//! Which instrument RackForge opens on when a session has no saved choice.

/// The instrument a performer meets on a fresh install.
pub const DEFAULT_INSTRUMENT_ID: &str = "org.rackforge.concert-grand";

/// Chooses the instrument to open PLAY on, preferring the Concert Grand.
///
/// Every host discovers its packages in some incidental order — a map keyed
/// by plugin id on Desktop, sorted paths in the browser — and simply taking
/// the first one meant the instrument that greeted a first-time performer was
/// whichever id happened to sort earliest. It was the Concert Grand by
/// accident, and only for as long as no installed package sorted ahead of it.
///
/// Hosts that carry no Concert Grand — the Minimal edition ships no
/// instruments at all — still open on whatever they do have.
pub fn choose_opening_instrument<T>(plugins: &[T], plugin_id: impl Fn(&T) -> &str) -> Option<&T> {
    plugins
        .iter()
        .find(|plugin| plugin_id(plugin) == DEFAULT_INSTRUMENT_ID)
        .or_else(|| plugins.first())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    fn chosen(plugins: &[String]) -> Option<&str> {
        choose_opening_instrument(plugins, |plugin| plugin.as_str()).map(String::as_str)
    }

    /// The Concert Grand is the instrument RackForge ships, so it is the one a
    /// fresh session opens on however the packages happened to be ordered.
    #[test]
    fn the_concert_grand_opens_wherever_it_sits_in_the_list() {
        assert_eq!(
            chosen(&ids(&[
                "org.rackforge.rf-106",
                "org.rackforge.concert-grand",
                "org.rackforge.rf-5",
            ])),
            Some("org.rackforge.concert-grand")
        );
    }

    /// A package installed under an id that sorts ahead of the Concert Grand
    /// used to take PLAY on the next start. Discovery order no longer decides.
    #[test]
    fn a_package_sorting_first_no_longer_takes_play() {
        assert_eq!(
            chosen(&ids(&["com.example.arp", "org.rackforge.concert-grand"])),
            Some("org.rackforge.concert-grand")
        );
    }

    /// Minimal carries no instruments of its own, so whatever a performer
    /// installed is what opens.
    #[test]
    fn without_the_concert_grand_the_first_instrument_opens() {
        assert_eq!(
            chosen(&ids(&["org.rackforge.rf-106", "org.rackforge.rf-5"])),
            Some("org.rackforge.rf-106")
        );
    }

    #[test]
    fn nothing_opens_when_nothing_is_installed() {
        assert_eq!(chosen(&[]), None);
    }
}
