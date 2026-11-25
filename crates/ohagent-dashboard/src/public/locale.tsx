import { createContext, useContext, useEffect, useState, type ReactNode } from 'react';

export type Language = 'en' | 'lv' | 'ru';
export const LANGUAGE_KEY = 'ohagent-language';
export const LANGUAGE_EVENT = 'ohagent-language';
const en = {
  clientReference: 'Client reference', portfolioProduct: 'Product', visitWebsite: 'Visit website', websiteNotPublished: 'Public website not listed',
  celotletiDescription: 'Flight search and airline ticket booking. Celotleti.lv is a reference for our work on travel booking systems.',
  celotletiUseCase: 'For travellers finding flights and booking airline tickets.',
  flatlyDescription: 'Property and household management: buildings, bills, meter readings, documents and communication with residents.',
  flatlyUseCase: 'For property managers, apartment owners and residents.',
  accountingDescription: 'An AI-powered accounting platform for freelancers, microbusinesses and small companies. Focused on business documents and bookkeeping workflows.',
  accountingUseCase: 'For small businesses and the people managing their accounts.',
  karminaDescription: 'A platform for animation and photorealistic filmmaking with an AI team of specialized agents, each focused on its own niche, under the direction of the author and director.',
  karminaUseCase: 'For authors and directors leading an AI filmmaking team while retaining creative control.',
  medicineDescription: 'Medication reminders in Telegram. Helps people keep track of scheduled medication and daily intake.',
  medicineUseCase: 'For people who want a practical way to manage medication reminders.',
  pilotCode: 'Pilot code, not deployed', availabilityUnverified: 'Availability not verified', sourceAvailable: 'Public source available', ohUseCase: 'For teams turning supplied materials into analytical briefs.', bugUseCase: 'For teams maintaining application regression coverage.', graphUseCase: 'For engineers exploring code relationships.',
  skip: 'Skip to content', home: 'Home', products: 'Products', approach: 'Our approach', contact: 'Contact', operator: 'Operator console',
  language: 'Language', product: 'Agent as a Service', explore: 'Explore the platform', demo: 'Explore the demo', openApp: 'Open workspace',
  eyebrow: 'ORANGEHAT / APPLIED AGENT SYSTEMS', hero: 'Specialist agents.\nShared intelligence.',
  intro: 'Turn domain knowledge into purposeful agents. OrangeHat brings the agent definition, its tools and its runtime into one considered system.',
  heroNote: 'Built around ohAgent. Designed for domain-specific work.', catalogTitle: 'A focused product ecosystem.',
  catalogIntro: 'Our products and client references span travel, property management, accounting, medication reminders and filmmaking, alongside our agent runtime and open-source tooling.',
  catalog: 'Product ecosystem', runtime: 'Runtime', publicProduct: 'Public product', openSource: 'Open-source tooling',
  ohDescription: 'The OrangeHat runtime for portable, domain-specific Standard Agents. It orchestrates Jcode, sessions, memory and tools.',
  bugDescription: 'Application intelligence for autonomous regression. Its public site describes semantic application scanning and maintainable regression suites.',
  graphDescription: 'An open-source code knowledge graph. Its public repository documents local AST parsing and queryable code relationships.',
  internalEvidence: 'Evidence: this repository', publicEvidence: 'View public source', checked: 'Source reviewed', evidenceNote: 'A source link verifies the description, not ownership, availability, integration or performance. No affiliation with third-party projects is implied.',
  approachTitle: 'Domain expertise, without a dead end.', approachIntro: 'A reusable agent is more than a prompt. Keep its purpose explicit, its execution inspectable and its boundaries clear.',
  define: 'Define the specialist', defineBody: 'Describe the domain, permitted tools and expected outputs as an agent definition.',
  compose: 'Compose the context', composeBody: 'Connect relevant skills and knowledge to a purpose-built agent, rather than one universal assistant.',
  run: 'Run with boundaries', runBody: 'Use ohAgent as the runtime. Keep a definition, a workflow stage and an actual running instance distinct.',
  standardNote: 'Standard Agent Specification conformance has not yet been verified. Portability is a design goal, not a certification claim.',
  contactTitle: 'Start with the work that matters.', contactBody: 'Explore the product and its clearly labelled demonstration before deciding how it could fit your domain.',
  contactUnavailable: 'A verified company contact is not available in this release. No enquiry is sent from this website.',
  footer: 'Purpose-built agents. Considered systems.', privacy: 'No contact form or marketing trackers on these public pages.',
  productEyebrow: 'AGENT AS A SERVICE / POWERED BY OHAGENT', productHero: 'Your domain.\nYour agent.\nYour control.',
  productIntro: 'Turn your source materials into an analytical brief. A coordinator plans the work, an analyst and evidence checker run in parallel, and a reviewer merges their findings on the shared ohAgent runtime. The pilot accepts text only and has no external tools.',
  productNote: 'Explore a local demonstration, or open the product workspace. The demo does not execute real agents.',
  definition: 'Agent definition', definitionSub: 'Purpose · knowledge · tools', runtimeLabel: 'ohAgent runtime', runtimeSub: 'Sessions · memory · orchestration', instance: 'Runtime instance', instanceSub: 'An execution, not a definition',
  domainLabel: 'Domain-specific agents', domainOne: 'Engineering', domainTwo: 'Operations', domainThree: 'Knowledge', diagramLabel: 'Conceptual architecture, not a live system',
  capabilities: 'From definition to execution.', capabilitiesIntro: 'A clear separation between what an agent is, where it runs and what it did.',
  definitionBody: 'A portable description of a specialist and its scope. Reuse the definition without confusing it with a running process.',
  runtimeBody: 'ohAgent connects Jcode execution with sessions, memory and tools. The runtime is not a replacement agent engine.',
  instanceBody: 'A specific run belongs to its own execution context. A workflow stage is not itself an agent instance.',
  demoBadge: 'LOCAL DEMO · SAMPLE DATA', demoTitle: 'A specialist, step by step.', demoIntro: 'An interactive illustration using fixed sample data. No backend, credentials or live agent execution. Nothing entered here is sent to a server.',
  sampleAgent: 'Analytical brief specialists', sampleTask: 'Summarize a sample project update', sampleDefinition: 'Coordinator defines the scope, analyst reviews the supplied text, and reviewer checks source quotations. This walkthrough is illustrative and does not run.',
  demoStart: 'Run sample walkthrough', demoNext: 'Next sample step', demoReset: 'Reset sample', sampleReady: 'Ready to explore', sampleComplete: 'Sample walkthrough complete',
  sampleStepOne: 'Definition selected', sampleStepTwo: 'Sample context attached', sampleStepThree: 'Illustrative review prepared',
  sampleResult: 'Sample result: the project update needs a confirmed delivery date. This is fixed demonstration text, not an AI-generated finding.',
  demoBack: 'Back to product', demoDisclaimer: 'This demonstration is not evidence of runtime availability, standards conformance or production readiness.',
  unavailableTitle: 'Workspace is not included in this build', unavailableBody: 'The product workspace module has not been delivered here yet. You can still explore the local demo.',
  loading: 'Loading workspace…', notFound: 'Page not found', notFoundBody: 'This address does not match a public page or an operator tool.',
  dashboard: 'Dashboard', skills: 'Skills', memory: 'Memory', keys: 'API keys', usage: 'Usage', progress: 'Live activity',
  operatorNote: 'Operator tools. This is not customer sign-in.', learnMore: 'Learn more', sample: 'Sample', architecture: 'Architecture', availability: 'Availability and pricing are not published here.',
};
export type Copy = { [K in keyof typeof en]: string };
const lv: Copy = {
  clientReference: 'Klienta projekts', portfolioProduct: 'Produkts', visitWebsite: 'Apmeklēt vietni', websiteNotPublished: 'Publiska vietne nav norādīta',
  celotletiDescription: 'Lidojumu meklēšana un aviobiļešu rezervēšana. Celotleti.lv ir piemērs mūsu darbam ceļojumu rezervēšanas sistēmu izstrādē.',
  celotletiUseCase: 'Ceļotājiem, kuri meklē lidojumus un rezervē aviobiļetes.',
  flatlyDescription: 'Īpašumu un mājsaimniecību pārvaldība: ēkas, rēķini, skaitītāju rādījumi, dokumenti un saziņa ar iedzīvotājiem.',
  flatlyUseCase: 'Īpašumu pārvaldniekiem, dzīvokļu īpašniekiem un iedzīvotājiem.',
  accountingDescription: 'Ar MI darbināta grāmatvedības platforma pašnodarbinātajiem, mikrouzņēmumiem un maziem uzņēmumiem. Uzmanības centrā ir uzņēmuma dokumenti un grāmatvedības procesi.',
  accountingUseCase: 'Maziem uzņēmumiem un to grāmatvedības kārtotājiem.',
  karminaDescription: 'Platforma animācijas un fotoreālistiska kino veidošanai ar specializētu MI aģentu komandu autora un režisora vadībā. Katrs aģents darbojas savā nišā.',
  karminaUseCase: 'Autoriem un režisoriem, kuri vada MI kino veidošanas komandu un saglabā radošo kontroli.',
  medicineDescription: 'Atgādinājumi par zāļu lietošanu Telegram lietotnē. Palīdz sekot zāļu lietošanas grafikam un ikdienas devām.',
  medicineUseCase: 'Cilvēkiem, kuri vēlas ērti pārvaldīt atgādinājumus par zāļu lietošanu.',
  pilotCode: 'Pilota kods, nav izvietots', availabilityUnverified: 'Pieejamība nav pārbaudīta', sourceAvailable: 'Publisks pirmkods', ohUseCase: 'Komandām, kas veido analītiskus pārskatus no saviem materiāliem.', bugUseCase: 'Komandām, kas uztur lietotņu regresijas pārklājumu.', graphUseCase: 'Inženieriem koda saišu izpētei.',
  skip: 'Pāriet uz saturu', home: 'Sākums', products: 'Produkti', approach: 'Mūsu pieeja', contact: 'Kontakti', operator: 'Operatora konsole', language: 'Valoda', product: 'Aģents kā pakalpojums', explore: 'Iepazīt platformu', demo: 'Izpētīt demonstrāciju', openApp: 'Atvērt darbvietu',
  eyebrow: 'ORANGEHAT / LIETIŠĶĀS AĢENTU SISTĒMAS', hero: 'Specializēti aģenti.\nKopīga inteliģence.', intro: 'Pārvērtiet nozares zināšanas mērķtiecīgos aģentos. OrangeHat apvieno aģenta definīciju, rīkus un izpildvidi pārdomātā sistēmā.', heroNote: 'Pamatā ohAgent. Veidots konkrētu nozaru uzdevumiem.',
  catalogTitle: 'Mērķtiecīga produktu ekosistēma.', catalogIntro: 'Mūsu produkti un klientu projekti aptver ceļojumus, īpašumu pārvaldību, grāmatvedību, zāļu lietošanas atgādinājumus un kino veidošanu līdzās aģentu izpildvidei un atvērtā pirmkoda rīkiem.', catalog: 'Produktu ekosistēma', runtime: 'Izpildvide', publicProduct: 'Publisks produkts', openSource: 'Atvērtā pirmkoda rīki',
  ohDescription: 'OrangeHat izpildvide pārnesamiem, nozarei pielāgotiem Standard Agents. Tā koordinē Jcode, sesijas, atmiņu un rīkus.', bugDescription: 'Lietotņu analīze autonomai regresijas testēšanai. Publiskā vietne apraksta semantisku lietotņu skenēšanu un uzturamas regresijas testu kopas.', graphDescription: 'Atvērtā pirmkoda koda zināšanu grafs. Publiskā repozitorija dokumentācija apraksta lokālu AST analīzi un vaicājamas koda saites.', internalEvidence: 'Avots: šis repozitorijs', publicEvidence: 'Skatīt publisko avotu', checked: 'Avots pārskatīts', evidenceNote: 'Avota saite pamato aprakstu, nevis īpašumtiesības, pieejamību, integrāciju vai veiktspēju. Saistība ar trešo pušu projektiem netiek apgalvota.',
  approachTitle: 'Nozares pieredze bez strupceļa.', approachIntro: 'Atkārtoti izmantojams aģents ir kas vairāk par uzvedni. Tā mērķim, izpildei un robežām jābūt skaidriem.', define: 'Definējiet speciālistu', defineBody: 'Aprakstiet nozari, atļautos rīkus un sagaidāmos rezultātus aģenta definīcijā.', compose: 'Apvienojiet kontekstu', composeBody: 'Pievienojiet vajadzīgās prasmes un zināšanas konkrētam mērķim, nevis vienam universālam asistentam.', run: 'Izpildiet noteiktās robežās', runBody: 'Izmantojiet ohAgent kā izpildvidi. Nošķiriet definīciju, darbplūsmas posmu un faktisko izpildes instanci.', standardNote: 'Atbilstība Standard Agent Specification vēl nav pārbaudīta. Pārnesamība ir izstrādes mērķis, nevis sertifikācija.',
  contactTitle: 'Sāciet ar svarīgāko darbu.', contactBody: 'Iepazīstiet produktu un skaidri marķēto demonstrāciju, lai izvērtētu piemērotību savai nozarei.', contactUnavailable: 'Šajā laidienā nav pieejams pārbaudīts uzņēmuma kontakts. Šī vietne nesūta pieprasījumus.', footer: 'Mērķtiecīgi aģenti. Pārdomātas sistēmas.', privacy: 'Šajās publiskajās lapās nav kontaktformas vai mārketinga izsekotāju.',
  productEyebrow: 'AĢENTS KĀ PAKALPOJUMS / DARBINA OHAGENT', productHero: 'Jūsu nozare.\nJūsu aģents.\nJūsu kontrole.', productIntro: 'Pārvērtiet savus avota materiālus analītiskā pārskatā. Koordinators plāno darbu, analītiķis un avotu pārbaudītājs strādā paralēli, pārskatītājs apvieno secinājumus kopīgajā ohAgent izpildvidē. Pilots pieņem tikai tekstu un neizmanto ārējus rīkus.', productNote: 'Izmēģiniet lokālu demonstrāciju vai atveriet produkta darbvietu. Demonstrācija neizpilda īstus aģentus.',
  definition: 'Aģenta definīcija', definitionSub: 'Mērķis · zināšanas · rīki', runtimeLabel: 'ohAgent izpildvide', runtimeSub: 'Sesijas · atmiņa · koordinēšana', instance: 'Izpildes instance', instanceSub: 'Izpilde, nevis definīcija', domainLabel: 'Nozaru aģenti', domainOne: 'Izstrāde', domainTwo: 'Operācijas', domainThree: 'Zināšanas', diagramLabel: 'Konceptuāla arhitektūra, nevis aktīva sistēma',
  capabilities: 'No definīcijas līdz izpildei.', capabilitiesIntro: 'Skaidra atšķirība starp aģentu, tā izpildvidi un paveikto.', definitionBody: 'Pārnesams speciālista un tā robežu apraksts. Atkārtoti izmantojiet definīciju, nejaucot to ar strādājošu procesu.', runtimeBody: 'ohAgent savieno Jcode izpildi ar sesijām, atmiņu un rīkiem. Izpildvide neaizstāj aģentu dzinēju.', instanceBody: 'Konkrētai izpildei ir savs konteksts. Darbplūsmas posms pats par sevi nav aģenta instance.',
  demoBadge: 'LOKĀLA DEMONSTRĀCIJA · PARAUGDATI', demoTitle: 'Speciālists soli pa solim.', demoIntro: 'Interaktīvs piemērs ar fiksētiem paraugdatiem. Bez servera, piekļuves datiem vai īstu aģentu izpildes. Nekas netiek nosūtīts serverim.', sampleAgent: 'Analītiskā pārskata speciālisti', sampleTask: 'Apkopot projekta ziņojuma piemēru', sampleDefinition: 'Koordinators nosaka robežas, analītiķis pārskata iesniegto tekstu, pārskatītājs pārbauda avota citātus. Šis ir ilustratīvs piemērs bez izpildes.', demoStart: 'Palaist piemēra apskatu', demoNext: 'Nākamais piemēra solis', demoReset: 'Atiestatīt piemēru', sampleReady: 'Gatavs apskatei', sampleComplete: 'Piemēra apskats pabeigts', sampleStepOne: 'Izvēlēta definīcija', sampleStepTwo: 'Pievienots piemēra konteksts', sampleStepThree: 'Sagatavots ilustratīvs pārskats', sampleResult: 'Piemēra rezultāts: projekta ziņojumam vajadzīgs apstiprināts piegādes datums. Šis ir fiksēts demonstrācijas teksts, nevis MI atradums.', demoBack: 'Atpakaļ uz produktu', demoDisclaimer: 'Demonstrācija neapliecina izpildvides pieejamību, atbilstību standartiem vai gatavību produkcijai.',
  unavailableTitle: 'Šajā būvē nav iekļauta darbvieta', unavailableBody: 'Produkta darbvietas modulis vēl nav piegādāts. Jūs joprojām varat izpētīt lokālo demonstrāciju.', loading: 'Ielādē darbvietu…', notFound: 'Lapa nav atrasta', notFoundBody: 'Šī adrese neatbilst publiskai lapai vai operatora rīkam.', dashboard: 'Pārskats', skills: 'Prasmes', memory: 'Atmiņa', keys: 'API atslēgas', usage: 'Lietojums', progress: 'Darba gaita', operatorNote: 'Operatora rīki. Šī nav klientu pierakstīšanās.', learnMore: 'Uzzināt vairāk', sample: 'Piemērs', architecture: 'Arhitektūra', availability: 'Pieejamība un cenas šeit nav publicētas.',
};
const ru: Copy = {
  clientReference: 'Клиентский проект', portfolioProduct: 'Продукт', visitWebsite: 'Открыть сайт', websiteNotPublished: 'Публичный сайт не указан',
  celotletiDescription: 'Поиск рейсов и бронирование авиабилетов. Celotleti.lv представляет наш опыт разработки систем бронирования путешествий.',
  celotletiUseCase: 'Для путешественников, которые ищут рейсы и бронируют авиабилеты.',
  flatlyDescription: 'Управление недвижимостью и домохозяйствами: здания, счета, показания счётчиков, документы и общение с жильцами.',
  flatlyUseCase: 'Для управляющих недвижимостью, собственников квартир и жильцов.',
  accountingDescription: 'Платформа бухгалтерского учёта с ИИ для самозанятых, микропредприятий и малого бизнеса. Работа с документами компании и бухгалтерскими процессами.',
  accountingUseCase: 'Для небольших компаний и специалистов, ведущих их учёт.',
  karminaDescription: 'Платформа для создания анимационного и фотореалистичного кино с AI-командой специализированных агентов под управлением автора и режиссёра. Каждый агент заточен под свою нишу.',
  karminaUseCase: 'Для авторов и режиссёров, которые руководят AI-командой создания кино и сохраняют творческий контроль.',
  medicineDescription: 'Напоминания о приёме лекарств в Telegram. Помогает следить за расписанием приёма и ежедневными дозами.',
  medicineUseCase: 'Для людей, которым нужен удобный способ управлять напоминаниями о лекарствах.',
  pilotCode: 'Код пилота, не развёрнут', availabilityUnverified: 'Доступность не проверена', sourceAvailable: 'Открытый исходный код', ohUseCase: 'Для команд, готовящих аналитические записки по своим материалам.', bugUseCase: 'Для команд, поддерживающих регрессионное покрытие приложений.', graphUseCase: 'Для инженеров, изучающих связи в коде.',
  skip: 'Перейти к содержимому', home: 'Главная', products: 'Продукты', approach: 'Наш подход', contact: 'Контакты', operator: 'Консоль оператора', language: 'Язык', product: 'Агент как услуга', explore: 'Изучить платформу', demo: 'Посмотреть демо', openApp: 'Открыть рабочую область',
  eyebrow: 'ORANGEHAT / ПРИКЛАДНЫЕ АГЕНТНЫЕ СИСТЕМЫ', hero: 'Специализированные агенты.\nОбщий интеллект.', intro: 'Превратите отраслевые знания в целевых агентов. OrangeHat объединяет определение агента, его инструменты и среду исполнения в продуманную систему.', heroNote: 'На базе ohAgent. Для задач конкретной предметной области.',
  catalogTitle: 'Целевая экосистема продуктов.', catalogIntro: 'Наши продукты и клиентские проекты охватывают путешествия, управление недвижимостью, бухгалтерию, напоминания о лекарствах и создание кино наряду со средой агентов и открытыми инструментами.', catalog: 'Экосистема продуктов', runtime: 'Среда исполнения', publicProduct: 'Публичный продукт', openSource: 'Открытые инструменты',
  ohDescription: 'Среда OrangeHat для переносимых отраслевых Standard Agents. Координирует Jcode, сессии, память и инструменты.', bugDescription: 'Анализ приложений для автономного регрессионного тестирования. Публичный сайт описывает семантическое сканирование и поддерживаемые наборы регрессионных тестов.', graphDescription: 'Открытый граф знаний о коде. Публичный репозиторий описывает локальный разбор AST и связи кода, доступные для запросов.', internalEvidence: 'Источник: этот репозиторий', publicEvidence: 'Открыть публичный источник', checked: 'Источник проверен', evidenceNote: 'Ссылка подтверждает описание, но не владение, доступность, интеграцию или производительность. Связь со сторонними проектами не заявляется.',
  approachTitle: 'Отраслевые знания без тупиков.', approachIntro: 'Многоразовый агент больше, чем промпт. Его цель, исполнение и границы должны быть понятными.', define: 'Определите специалиста', defineBody: 'Опишите предметную область, разрешённые инструменты и ожидаемые результаты в определении агента.', compose: 'Соберите контекст', composeBody: 'Подключите нужные навыки и знания к целевому агенту вместо одного универсального помощника.', run: 'Исполняйте в заданных границах', runBody: 'Используйте ohAgent как среду исполнения. Различайте определение, этап процесса и реальный запущенный экземпляр.', standardNote: 'Соответствие Standard Agent Specification пока не проверено. Переносимость является целью разработки, а не заявлением о сертификации.',
  contactTitle: 'Начните с важной задачи.', contactBody: 'Изучите продукт и явно обозначенную демонстрацию, чтобы оценить их применимость в вашей области.', contactUnavailable: 'Проверенный контакт компании в этом выпуске недоступен. Сайт не отправляет обращения.', footer: 'Целевые агенты. Продуманные системы.', privacy: 'На публичных страницах нет контактной формы или маркетинговых трекеров.',
  productEyebrow: 'АГЕНТ КАК УСЛУГА / НА БАЗЕ OHAGENT', productHero: 'Ваша область.\nВаш агент.\nВаш контроль.', productIntro: 'Превратите свои исходные материалы в аналитическую записку. Координатор планирует работу, аналитик и проверяющий источники действуют параллельно, рецензент объединяет их выводы в общей среде ohAgent. Пилот принимает только текст и не использует внешние инструменты.', productNote: 'Изучите локальную демонстрацию или откройте рабочую область продукта. Демо не запускает реальных агентов.',
  definition: 'Определение агента', definitionSub: 'Цель · знания · инструменты', runtimeLabel: 'Среда ohAgent', runtimeSub: 'Сессии · память · координация', instance: 'Экземпляр исполнения', instanceSub: 'Запуск, а не определение', domainLabel: 'Отраслевые агенты', domainOne: 'Разработка', domainTwo: 'Операции', domainThree: 'Знания', diagramLabel: 'Концептуальная архитектура, не работающая система',
  capabilities: 'От определения к исполнению.', capabilitiesIntro: 'Чёткое различие между тем, что представляет собой агент, где он работает и что он сделал.', definitionBody: 'Переносимое описание специалиста и его границ. Используйте определение повторно, не путая его с запущенным процессом.', runtimeBody: 'ohAgent связывает исполнение Jcode с сессиями, памятью и инструментами. Среда не заменяет агентный движок.', instanceBody: 'Каждый запуск имеет свой контекст исполнения. Этап рабочего процесса сам по себе не является экземпляром агента.',
  demoBadge: 'ЛОКАЛЬНОЕ ДЕМО · ПРИМЕРЫ ДАННЫХ', demoTitle: 'Специалист, шаг за шагом.', demoIntro: 'Интерактивный пример с фиксированными данными. Без сервера, учётных данных и реального исполнения агентов. Данные не отправляются на сервер.', sampleAgent: 'Специалисты по аналитической записке', sampleTask: 'Обобщить пример отчёта о проекте', sampleDefinition: 'Координатор задаёт рамки, аналитик изучает предоставленный текст, рецензент проверяет цитаты из источника. Это иллюстрация без реального исполнения.', demoStart: 'Запустить пример', demoNext: 'Следующий шаг примера', demoReset: 'Сбросить пример', sampleReady: 'Готово к просмотру', sampleComplete: 'Просмотр примера завершён', sampleStepOne: 'Выбрано определение', sampleStepTwo: 'Добавлен пример контекста', sampleStepThree: 'Подготовлен иллюстративный обзор', sampleResult: 'Пример результата: в отчёте о проекте не хватает подтверждённой даты поставки. Это фиксированный демонстрационный текст, а не вывод ИИ.', demoBack: 'Назад к продукту', demoDisclaimer: 'Демонстрация не подтверждает доступность среды, соответствие стандартам или готовность к промышленной эксплуатации.',
  unavailableTitle: 'Рабочая область не включена в эту сборку', unavailableBody: 'Модуль рабочей области продукта ещё не поставлен. Вы можете изучить локальное демо.', loading: 'Загрузка рабочей области…', notFound: 'Страница не найдена', notFoundBody: 'Этот адрес не соответствует публичной странице или инструменту оператора.', dashboard: 'Обзор', skills: 'Навыки', memory: 'Память', keys: 'Ключи API', usage: 'Использование', progress: 'Ход работы', operatorNote: 'Инструменты оператора. Это не вход для клиентов.', learnMore: 'Подробнее', sample: 'Пример', architecture: 'Архитектура', availability: 'Доступность и цены здесь не опубликованы.',
};
export const dictionaries: Record<Language, Copy> = { en, lv, ru };
export function isLanguage(value: unknown): value is Language { return value === 'en' || value === 'lv' || value === 'ru'; }
export function readLanguage(): Language {
  try { const value = localStorage.getItem(LANGUAGE_KEY); if (isLanguage(value)) return value; } catch { /* Storage may be disabled. */ }
  const browser = navigator.language.slice(0, 2);
  return isLanguage(browser) ? browser : 'en';
}
const LocaleContext = createContext<{ language: Language; setLanguage: (value: Language) => void; t: Copy }>({ language: 'en', setLanguage: () => {}, t: en });
export function LocaleProvider({ children }: { children: ReactNode }) {
  const [language, updateLanguage] = useState<Language>(readLanguage);
  useEffect(() => {
    const sync = (event?: Event) => {
      const detail: unknown = event instanceof CustomEvent ? event.detail : undefined;
      updateLanguage(isLanguage(detail) ? detail : readLanguage());
    };
    const storage = (event: StorageEvent) => { if (event.key === LANGUAGE_KEY || event.key === null) sync(); };
    window.addEventListener(LANGUAGE_EVENT, sync);
    window.addEventListener('storage', storage);
    return () => { window.removeEventListener(LANGUAGE_EVENT, sync); window.removeEventListener('storage', storage); };
  }, []);
  useEffect(() => { document.documentElement.lang = language; }, [language]);
  const setLanguage = (value: Language) => {
    updateLanguage(value);
    try { localStorage.setItem(LANGUAGE_KEY, value); } catch { /* Keep the current session usable without storage. */ }
    window.dispatchEvent(new CustomEvent(LANGUAGE_EVENT, { detail: value }));
  };
  return <LocaleContext.Provider value={{ language, setLanguage, t: dictionaries[language] }}>{children}</LocaleContext.Provider>;
}
export const useLocale = () => useContext(LocaleContext);
