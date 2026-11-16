# Parapet

Przytulna, przezroczysta i „przeklikiwalna” nakładka na cały pulpit Windows: jesienne liście, deszcz,
śnieg i burza, które reagują na okna, kursor myszy i prawdziwą pogodę za oknem.

## Co potrafi

- **Liście**: 28 gatunków (klony, dęby, buk, brzoza, lipa, miłorząb, kasztan, głóg…) w ciepłych, płaskich
  kolorach. Każdy liść spada po swojemu: kołysze się jak wahadło, koziołkuje albo opada spiralą,
  co jakiś czas zmienia styl i ma własne zawirowania.
- **Okna to daszki**: liście i śnieg osiadają na górnych krawędziach okien i jadą razem z oknem.
  Szarpnięcie oknem je strząsa. Deszcz spływa do narożników, gdzie zbiera się kropla, która spada
  jak z rynny, a część wody spływa po bocznych krawędziach okna.
- **Pasek zadań to ziemia**: liście, zaspy śniegu, a po deszczu błoto i kałuże z falami i kręgami
  od kropli. Działa też przy zmaksymalizowanym oknie.
- **Mysz to wiatr**: ruch kursora porywa spadające liście i płatki, podrywa leżące liście, zdmuchuje śnieg
  z zasp, a w kałużach robi fale i chlapie wodą lub błotem.
- **Rzadkie zjawiska** pojawiają się same co kilka minut, zależnie od pogody (menu → Rzadkie zjawiska,
  można je też przywołać ręcznie):
  jeż nabijający liście na kolce (zwija się w kulkę przy kursorze), gąsienica zjadająca leżące liście,
  lis polarny nurkujący w zaspy, przebiśniegi, gdy się ociepla, ślimak ze śladem śluzu po deszczu,
  żaba skacząca po kałużach, rudzik przysiadający na oknach, wiewiórka z żołędziem, grzyby w błocie.

## Pogoda

Tryb **Automatycznie** co 15 min pobiera pogodę z Open-Meteo (bez klucza API): śnieg przy opadzie ≤ 0,5°C,
deszcz przy deszczu lub mżawce (jesienią z liśćmi), błyski przy burzy, liście, gdy nie pada. Kierunek
i siła wiatru znoszą cząsteczki. Lokalizacja: z IP albo z `City=` lub `Latitude=`/`Longitude=`
w `bin\settings.ini` (menu → Ustawienia). Zmiany pliku są wczytywane automatycznie.

## Jakość i wydajność

Menu → Wydajność: 20/30/60 kl/s oraz jakość **niska** (½ rozdzielczości), **średnia** (¾), **wysoka**
(pełna) i **ultra** (pełna + cienie liści).

- Natywny C++, Win32, ok. 300 KB exe, bez runtime'u.
- Rysuje GPU: Direct2D + DirectComposition. Liście, krople, płatki, a także warstwy śniegu, błota i wody
  to sprite'y z jednego atlasu. Liście są malowane w 4 rozmiarach (mipmapy), żeby nie migotały.
- Okna śledzone przez zdarzenia systemowe, nie przez ciągłe odpytywanie. Kursor odczytywany raz na klatkę,
  bez globalnego hooka myszy.
- Pauza przy grach/filmach na pełnym ekranie, zablokowanym ekranie i wyłączonym monitorze. Na baterii
  maks. 20 kl/s i EcoQoS. Menu w zasobniku nie zatrzymuje animacji.

Zmierzone na 4K + drugi monitor:

| Scenariusz | CPU (jednego rdzenia) | RAM |
|---|---|---|
| Burza z kałużami, wysoka, 30 kl/s | 2,6% | 18 MB |
| Śnieg z zaspami, wysoka, 30 kl/s | 3,5% | 21 MB |
| Burza z kałużami, ultra, 60 kl/s | 5,9% | 27 MB |
| Liście, ultra, 60 kl/s | 5,3% | 15 MB |

## Budowanie

`build.bat` (VS Build Tools 2019 + Windows 10 SDK) tworzy `bin\Parapet.exe`.
`build.bat debug` buduje wersję diagnostyczną z logiem, zrzutem atlasu liści i przywołaniem
wszystkich zwierzaków po starcie.

## Pobieranie

Gotowy `Parapet.exe` (Windows 10/11, x64) jest w zakładce
[Releases](https://github.com/d0pawlus/Parapet/releases). Wystarczy go uruchomić; ustawienia trafiają do
`settings.ini` obok pliku exe. Wersje na macOS i Linuksa są w planach.

## Licencja

Copyright (C) 2026 Dominik Pawluś

Parapet jest wolnym oprogramowaniem na licencji [GNU GPL v3](LICENSE).
