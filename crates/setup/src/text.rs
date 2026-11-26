//! The setup's texts, in Kuro's languages. English is the key; the Windows display language
//! picks the column.

use std::sync::OnceLock;

const CODES: [&str; 7] = ["en", "es", "fr", "de", "it", "nb", "ko"];

#[rustfmt::skip]
const TEXT: &[[&str; 7]] = &[
    ["Kuro Setup", "Instalación de Kuro", "Installation de Kuro", "Kuro-Setup", "Installazione di Kuro", "Kuro-installasjon", "Kuro 설치"],
    ["Version {0}", "Versión {0}", "Version {0}", "Version {0}", "Versione {0}", "Versjon {0}", "버전 {0}"],
    ["Kuro is not installed on this computer.", "Kuro no está instalado en este equipo.", "Kuro n'est pas installé sur cet ordinateur.", "Kuro ist auf diesem Computer nicht installiert.", "Kuro non è installato su questo computer.", "Kuro er ikke installert på denne datamaskinen.", "이 컴퓨터에 Kuro가 설치되어 있지 않습니다."],
    ["Kuro {0} is installed in {1}", "Kuro {0} está instalado en {1}", "Kuro {0} est installé dans {1}", "Kuro {0} ist installiert in {1}", "Kuro {0} è installato in {1}", "Kuro {0} er installert i {1}", "Kuro {0}이(가) {1}에 설치되어 있습니다"],
    ["Kuro is open: it will be closed first.", "Kuro está abierto: se cerrará antes.", "Kuro est ouvert : il sera d'abord fermé.", "Kuro ist geöffnet und wird zuerst geschlossen.", "Kuro è aperto: verrà chiuso prima.", "Kuro er åpent og lukkes først.", "Kuro가 실행 중입니다. 먼저 종료합니다."],
    ["Installs Kuro for your user only, without administrator rights.", "Instala Kuro solo para tu usuario, sin permisos de administrador.", "Installe Kuro pour votre utilisateur uniquement, sans droits d'administrateur.", "Installiert Kuro nur für Ihren Benutzer, ohne Administratorrechte.", "Installa Kuro solo per il tuo utente, senza diritti di amministratore.", "Installerer Kuro bare for brukeren din, uten administratorrettigheter.", "관리자 권한 없이 현재 사용자에게만 Kuro를 설치합니다."],
    ["Kuro {0} will be updated to {1}. Your lists, settings and sessions are kept.", "Kuro {0} se actualizará a {1}. Tus listas, ajustes y sesiones se conservan.", "Kuro {0} sera mis à jour vers {1}. Vos listes, réglages et sessions sont conservés.", "Kuro {0} wird auf {1} aktualisiert. Listen, Einstellungen und Anmeldungen bleiben erhalten.", "Kuro {0} verrà aggiornato a {1}. Elenchi, impostazioni e sessioni vengono mantenuti.", "Kuro {0} oppdateres til {1}. Lister, innstillinger og økter beholdes.", "Kuro {0}을(를) {1}(으)로 업데이트합니다. 목록, 설정, 로그인은 유지됩니다."],
    ["This version is already installed. Installing it again repairs it.", "Esta versión ya está instalada. Instalarla de nuevo la repara.", "Cette version est déjà installée. La réinstaller la répare.", "Diese Version ist bereits installiert. Eine Neuinstallation repariert sie.", "Questa versione è già installata. Reinstallarla la ripara.", "Denne versjonen er allerede installert. Å installere den på nytt reparerer den.", "이 버전이 이미 설치되어 있습니다. 다시 설치하면 복구됩니다."],
    ["Kuro {0} is newer than this setup. You can go back to {1}; your lists are kept.", "Kuro {0} es más nuevo que este instalador. Puedes volver a {1}; tus listas se conservan.", "Kuro {0} est plus récent que ce programme d'installation. Vous pouvez revenir à {1} ; vos listes sont conservées.", "Kuro {0} ist neuer als dieses Setup. Sie können zu {1} zurückkehren; Ihre Listen bleiben erhalten.", "Kuro {0} è più recente di questa installazione. Puoi tornare a {1}; i tuoi elenchi vengono mantenuti.", "Kuro {0} er nyere enn denne installasjonen. Du kan gå tilbake til {1}; listene dine beholdes.", "Kuro {0}이(가) 이 설치 프로그램보다 최신입니다. {1}(으)로 되돌릴 수 있으며 목록은 유지됩니다."],
    ["Kuro {0} has changed your lists in a way that Kuro {1} cannot read, so going back is not possible. Nothing has been changed.", "Kuro {0} ha cambiado tus listas de una forma que Kuro {1} no puede leer, así que no se puede volver atrás. No se ha cambiado nada.", "Kuro {0} a modifié vos listes d'une façon que Kuro {1} ne peut pas lire : impossible de revenir en arrière. Rien n'a été modifié.", "Kuro {0} hat Ihre Listen so geändert, dass Kuro {1} sie nicht lesen kann. Ein Zurück ist nicht möglich. Es wurde nichts geändert.", "Kuro {0} ha modificato i tuoi elenchi in un modo che Kuro {1} non può leggere, quindi non è possibile tornare indietro. Non è stato modificato nulla.", "Kuro {0} har endret listene dine slik at Kuro {1} ikke kan lese dem, så det går ikke å gå tilbake. Ingenting er endret.", "Kuro {0}이(가) Kuro {1}에서 읽을 수 없는 방식으로 목록을 변경했으므로 되돌릴 수 없습니다. 아무것도 변경하지 않았습니다."],
    ["Desktop shortcut", "Acceso directo en el escritorio", "Raccourci sur le bureau", "Verknüpfung auf dem Desktop", "Collegamento sul desktop", "Snarvei på skrivebordet", "바탕 화면 바로 가기"],
    ["Open Kuro when done", "Abrir Kuro al terminar", "Ouvrir Kuro à la fin", "Kuro danach öffnen", "Apri Kuro al termine", "Åpne Kuro når det er ferdig", "완료 후 Kuro 열기"],
    ["Install", "Instalar", "Installer", "Installieren", "Installa", "Installer", "설치"],
    ["Update to {0}", "Actualizar a {0}", "Mettre à jour vers {0}", "Auf {0} aktualisieren", "Aggiorna a {0}", "Oppdater til {0}", "{0}(으)로 업데이트"],
    ["Reinstall", "Reinstalar", "Réinstaller", "Neu installieren", "Reinstalla", "Installer på nytt", "다시 설치"],
    ["Go back to {0}", "Volver a {0}", "Revenir à {0}", "Zurück zu {0}", "Torna a {0}", "Gå tilbake til {0}", "{0}(으)로 되돌리기"],
    ["Uninstall…", "Desinstalar…", "Désinstaller…", "Deinstallieren…", "Disinstalla…", "Avinstaller…", "제거…"],
    ["Close", "Cerrar", "Fermer", "Schließen", "Chiudi", "Lukk", "닫기"],
    ["Uninstall Kuro {0}", "Desinstalar Kuro {0}", "Désinstaller Kuro {0}", "Kuro {0} deinstallieren", "Disinstalla Kuro {0}", "Avinstaller Kuro {0}", "Kuro {0} 제거"],
    ["Kuro is closed and removed from this computer. Your lists, settings and sessions stay, so installing it again picks up where you left off.", "Kuro se cierra y se quita de este equipo. Tus listas, ajustes y sesiones se quedan, así que si lo vuelves a instalar sigues donde lo dejaste.", "Kuro est fermé et retiré de cet ordinateur. Vos listes, réglages et sessions restent : une nouvelle installation reprend là où vous en étiez.", "Kuro wird geschlossen und von diesem Computer entfernt. Listen, Einstellungen und Anmeldungen bleiben, sodass eine Neuinstallation dort weitermacht, wo Sie aufgehört haben.", "Kuro viene chiuso e rimosso da questo computer. Elenchi, impostazioni e sessioni restano, quindi reinstallandolo riprendi da dove avevi lasciato.", "Kuro lukkes og fjernes fra denne datamaskinen. Lister, innstillinger og økter blir værende, så en ny installasjon fortsetter der du slapp.", "Kuro를 종료하고 이 컴퓨터에서 제거합니다. 목록, 설정, 로그인은 남아 있으므로 다시 설치하면 이어서 사용할 수 있습니다."],
    ["Also delete my lists, settings and sessions", "Borrar también mis listas, ajustes y sesiones", "Supprimer aussi mes listes, réglages et sessions", "Auch meine Listen, Einstellungen und Anmeldungen löschen", "Elimina anche elenchi, impostazioni e sessioni", "Slett også lister, innstillinger og økter", "목록, 설정, 로그인도 삭제"],
    ["Lists not synced with AniList or MyAnimeList are lost. This cannot be undone.", "Las listas que no estén sincronizadas con AniList o MyAnimeList se pierden. No se puede deshacer.", "Les listes non synchronisées avec AniList ou MyAnimeList sont perdues. Action irréversible.", "Nicht mit AniList oder MyAnimeList synchronisierte Listen gehen verloren. Das kann nicht rückgängig gemacht werden.", "Gli elenchi non sincronizzati con AniList o MyAnimeList vanno persi. Non si può annullare.", "Lister som ikke er synkronisert med AniList eller MyAnimeList, går tapt. Dette kan ikke angres.", "AniList 또는 MyAnimeList와 동기화되지 않은 목록은 사라집니다. 되돌릴 수 없습니다."],
    ["Uninstall", "Desinstalar", "Désinstaller", "Deinstallieren", "Disinstalla", "Avinstaller", "제거"],
    ["Back", "Atrás", "Retour", "Zurück", "Indietro", "Tilbake", "뒤로"],
    ["Please wait…", "Un momento…", "Veuillez patienter…", "Bitte warten…", "Attendere…", "Vent litt…", "잠시 기다려 주세요…"],
    ["Kuro {0} is installed.", "Kuro {0} está instalado.", "Kuro {0} est installé.", "Kuro {0} ist installiert.", "Kuro {0} è installato.", "Kuro {0} er installert.", "Kuro {0}이(가) 설치되었습니다."],
    ["Kuro is uninstalled.", "Kuro está desinstalado.", "Kuro est désinstallé.", "Kuro ist deinstalliert.", "Kuro è disinstallato.", "Kuro er avinstallert.", "Kuro가 제거되었습니다."],
    ["Something went wrong", "Algo ha fallado", "Un problème est survenu", "Etwas ist schiefgelaufen", "Qualcosa è andato storto", "Noe gikk galt", "문제가 발생했습니다"],
    ["Details", "Detalles", "Détails", "Details", "Dettagli", "Detaljer", "자세히"],
    ["Kuro is still open from {0}. Close it and try again.", "Kuro sigue abierto desde {0}. Ciérralo y vuelve a intentarlo.", "Kuro est toujours ouvert depuis {0}. Fermez-le et réessayez.", "Kuro ist noch geöffnet (aus {0}). Schließen Sie es und versuchen Sie es erneut.", "Kuro è ancora aperto da {0}. Chiudilo e riprova.", "Kuro er fortsatt åpent fra {0}. Lukk det og prøv igjen.", "Kuro가 아직 {0}에서 실행 중입니다. 종료한 후 다시 시도하세요."],
    ["This setup was built without Kuro inside.", "Este instalador se creó sin Kuro dentro.", "Ce programme d'installation ne contient pas Kuro.", "Dieses Setup wurde ohne Kuro erstellt.", "Questa installazione è stata creata senza Kuro.", "Denne installasjonen ble laget uten Kuro.", "이 설치 프로그램에는 Kuro가 들어 있지 않습니다."],
    ["Could not write {0}: {1}", "No se pudo escribir {0}: {1}", "Impossible d'écrire {0} : {1}", "{0} konnte nicht geschrieben werden: {1}", "Impossibile scrivere {0}: {1}", "Kunne ikke skrive {0}: {1}", "{0}을(를) 쓸 수 없습니다: {1}"],
    ["Could not remove {0}: {1}", "No se pudo borrar {0}: {1}", "Impossible de supprimer {0} : {1}", "{0} konnte nicht entfernt werden: {1}", "Impossibile rimuovere {0}: {1}", "Kunne ikke fjerne {0}: {1}", "{0}을(를) 제거할 수 없습니다: {1}"],
];

fn column() -> usize {
    static COLUMN: OnceLock<usize> = OnceLock::new();
    *COLUMN.get_or_init(|| {
        let locale = sys_locale::get_locale().unwrap_or_default().to_lowercase();
        let lang = locale.split(['-', '_']).next().unwrap_or("");
        let lang = if matches!(lang, "no" | "nn") { "nb" } else { lang };
        CODES.iter().position(|c| *c == lang).unwrap_or(0)
    })
}

pub fn t(key: &str) -> String {
    tf(key, &[])
}

/// The text for `key`, with {0}, {1}… replaced by `args`.
pub fn tf(key: &str, args: &[&str]) -> String {
    let row = TEXT.iter().find(|r| r[0] == key);
    debug_assert!(row.is_some(), "no text for {key}");
    let mut s = row.map_or(key, |r| r[column()]).to_owned();
    for (i, a) in args.iter().enumerate() {
        s = s.replace(&format!("{{{i}}}"), a);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_language_has_every_text_and_placeholder() {
        for row in TEXT {
            for cell in row {
                assert!(!cell.is_empty(), "{}", row[0]);
                for i in 0..3 {
                    let p = format!("{{{i}}}");
                    assert_eq!(cell.contains(&p), row[0].contains(&p), "{cell}");
                }
            }
        }
    }
}
