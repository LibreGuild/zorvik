//! Word lists and tables for the dynamic variables. Kept compact and international;
//! `mod.rs` skips formatting this file so the lists stay packed.

// ── People ────────────────────────────────────────────────────────────────────

pub(super) const FIRST_NAMES: &[&str] = &[
    "James", "Mary", "John", "Patricia", "Robert", "Jennifer", "Michael", "Linda", "William", "Elizabeth", "David",
    "Barbara", "Richard", "Susan", "Joseph", "Jessica", "Thomas", "Sarah", "Daniel", "Emily", "Matthew", "Olivia",
    "Anthony", "Emma", "Ethan", "Chloe", "Noah", "Grace", "Liam", "Harper", "Lucas", "Amelia", "Oliver", "Charlotte",
    "Henry", "Isla", "Jack", "Ella", "Sofía", "Mateo", "Valentina", "Santiago", "Camila", "Diego", "Lucía", "Javier",
    "Isabella", "Alejandro", "Martina", "Carlos", "Gabriela", "Pablo", "João", "Maria", "Pedro", "Beatriz", "Gustavo",
    "Larissa", "Rafael", "Fernanda", "Louis", "Camille", "Hugo", "Léa", "Julien", "Manon", "Antoine", "Élodie",
    "Lukas", "Hannah", "Felix", "Lena", "Jonas", "Mia", "Maximilian", "Anna", "Leon", "Johanna", "Giulia", "Lorenzo",
    "Francesca", "Alessandro", "Chiara", "Marco", "Sven", "Astrid", "Lars", "Ingrid", "Freya", "Mats", "Sanne", "Daan",
    "Noor", "Aleksandr", "Olga", "Dmitri", "Natalia", "Ivan", "Katarzyna", "Piotr", "Zofia", "Tomáš", "Ivana", "Nikos",
    "Eleni", "Mehmet", "Elif", "Emre", "Zeynep", "Mohammed", "Fatima", "Omar", "Aisha", "Youssef", "Layla", "Karim",
    "Mariam", "Hassan", "Noa", "Yosef", "Dariush", "Shirin", "Aarav", "Priya", "Arjun", "Ananya", "Rohan", "Diya",
    "Vikram", "Sneha", "Rahul", "Kavya", "Aditi", "Ishaan", "Meera", "Wei", "Jing", "Hao", "Xin", "Yan", "Jun", "Mei",
    "Lei", "Haruto", "Yui", "Sota", "Aoi", "Ren", "Sakura", "Hiroshi", "Yuki", "Kenji", "Emi", "Min-jun", "Seo-yeon",
    "Ji-ho", "Ha-eun", "Minh", "Linh", "Somchai", "Budi", "Siti", "Maricel", "Chinedu", "Amara", "Kwame", "Ama",
    "Thabo", "Zanele", "Tendai", "Wanjiru", "Kofi", "Nia", "Oluwaseun", "Adaeze", "Sipho", "Abebe", "Fatou", "Siobhan",
    "Ciarán", "Aoife", "Niamh", "Callum", "Aroha", "Keanu", "Marta", "Andrés", "Inés", "Bruno", "Nadia", "Samir",
];

pub(super) const LAST_NAMES: &[&str] = &[
    "Smith", "Johnson", "Williams", "Brown", "Jones", "Miller", "Davis", "Wilson", "Anderson", "Taylor", "Moore",
    "Jackson", "Martin", "Thompson", "White", "Harris", "Clark", "Lewis", "Walker", "Hall", "Young", "King", "Wright",
    "Scott", "Green", "Baker", "Adams", "Nelson", "Hill", "Campbell", "Mitchell", "Roberts", "Carter", "Evans",
    "Turner", "Parker", "Collins", "Edwards", "Stewart", "Murphy", "Cook", "Kelly", "O'Brien", "O'Connor", "Walsh",
    "McCarthy", "MacDonald", "Fraser", "García", "Rodríguez", "Martínez", "Hernández", "López", "González",
    "Fernández", "Pérez", "Sánchez", "Ramírez", "Torres", "Flores", "Rivera", "Gómez", "Díaz", "Morales", "Ortiz",
    "Castillo", "Romero", "Vargas", "Jiménez", "Ruiz", "Navarro", "Silva", "Santos", "Oliveira", "Souza", "Costa",
    "Pereira", "Almeida", "Ferreira", "Carvalho", "Rocha", "Dubois", "Moreau", "Laurent", "Lefebvre", "Girard",
    "Bonnet", "Fontaine", "Rousseau", "Lambert", "Mercier", "Müller", "Schmidt", "Schneider", "Fischer", "Weber",
    "Meyer", "Wagner", "Becker", "Schulz", "Hoffmann", "Koch", "Richter", "Schäfer", "Neumann", "Zimmermann", "Rossi",
    "Russo", "Ferrari", "Esposito", "Bianchi", "Romano", "Colombo", "Ricci", "Marino", "Greco", "Conti", "de Jong",
    "Jansen", "de Vries", "van den Berg", "Bakker", "Visser", "Hansen", "Johansson", "Andersson", "Nielsen",
    "Larsen", "Karlsson", "Virtanen", "Korhonen", "Lindqvist", "Ivanov", "Petrov", "Smirnov", "Kowalski", "Nowak",
    "Wiśniewski", "Novák", "Horvat", "Kovačević", "Popescu", "Nagy", "Kovalenko", "Papadopoulos", "Georgiou",
    "Yılmaz", "Kaya", "Demir", "Çelik", "Şahin", "Haddad", "Khalil", "Mansour", "Nasser", "Saleh", "El-Sayed", "Aziz",
    "Cohen", "Levi", "Mizrahi", "Hosseini", "Rahimi", "Sharma", "Patel", "Singh", "Kumar", "Gupta", "Reddy", "Iyer",
    "Nair", "Das", "Mehta", "Chatterjee", "Joshi", "Rao", "Menon", "Wang", "Li", "Zhang", "Liu", "Chen", "Yang",
    "Huang", "Zhao", "Wu", "Zhou", "Xu", "Lin", "Sato", "Suzuki", "Takahashi", "Tanaka", "Watanabe", "Ito",
    "Yamamoto", "Nakamura", "Kobayashi", "Kim", "Park", "Choi", "Jung", "Kang", "Nguyen", "Tran", "Pham", "Hoang",
    "Reyes", "Cruz", "Bautista", "Wijaya", "Santoso", "Hidayat", "Okafor", "Adeyemi", "Mensah", "Okonkwo", "Mwangi",
    "Dlamini", "Nkosi", "Diallo", "Traoré", "Osei", "Banda", "Moyo", "Ndlovu", "Kamau", "Te Rangi",
];

pub(super) const NAME_PREFIXES: &[&str] = &["Mr.", "Mrs.", "Ms.", "Miss", "Mx.", "Dr.", "Prof."];
pub(super) const NAME_SUFFIXES: &[&str] = &["Jr.", "Sr.", "I", "II", "III", "IV", "V", "MD", "DDS", "PhD", "DVM", "CPA"];
pub(super) const GENDERS: &[&str] = &["female", "male", "non-binary"];

pub(super) const JOB_DESCRIPTORS: &[&str] = &[
    "Lead", "Senior", "Direct", "Corporate", "Dynamic", "Future", "Product", "National", "Regional", "District",
    "Central", "Global", "Customer", "Investor", "Internal", "International", "Legacy", "Forward", "Principal",
    "Chief", "Human", "Junior", "Staff", "Associate",
];
pub(super) const JOB_AREAS: &[&str] = &[
    "Solutions", "Program", "Brand", "Security", "Research", "Marketing", "Directives", "Implementation",
    "Integration", "Functionality", "Response", "Paradigm", "Tactics", "Identity", "Markets", "Group", "Division",
    "Applications", "Optimization", "Operations", "Infrastructure", "Intranet", "Communications", "Web", "Branding",
    "Quality", "Assurance", "Mobility", "Accounts", "Data", "Creative", "Configuration", "Accountability",
    "Interactions", "Factors", "Usability", "Metrics", "Platform", "Cloud", "Payments", "Compliance", "Growth",
];
pub(super) const JOB_TYPES: &[&str] = &[
    "Supervisor", "Associate", "Executive", "Liaison", "Officer", "Manager", "Engineer", "Specialist", "Director",
    "Coordinator", "Administrator", "Architect", "Analyst", "Designer", "Planner", "Orchestrator", "Technician",
    "Developer", "Producer", "Consultant", "Assistant", "Facilitator", "Agent", "Representative", "Strategist",
];

/// E.164 mobile formats: ISO country, calling code, national number patterns
/// (`#` any digit, `N` 2-9, `M` 1-9).
pub(super) const PHONE_FORMATS: &[(&str, &str, &[&str])] = &[
    ("US", "1", &["N##N######"]), ("CA", "1", &["N##N######"]), ("GB", "44", &["7M########"]),
    ("DE", "49", &["151########", "17#########"]), ("FR", "33", &["6########", "7########"]),
    ("ES", "34", &["6########"]), ("IT", "39", &["3#########"]), ("NL", "31", &["6M#######"]),
    ("IN", "91", &["9#########", "7#########"]), ("BR", "55", &["MM9########"]), ("MX", "52", &["55########"]),
    ("JP", "81", &["90########", "80########"]), ("KR", "82", &["10########"]), ("CN", "86", &["13#########"]),
    ("AU", "61", &["4########"]), ("SG", "65", &["9#######"]), ("ZA", "27", &["8M#######"]),
    ("NG", "234", &["80########"]), ("KE", "254", &["7########"]),
];

// ── Location ─────────────────────────────────────────────────────────────────

/// ISO 3166-1: alpha-2, alpha-3, English short name.
pub(super) const COUNTRIES: &[(&str, &str, &str)] = &[
    ("AD", "AND", "Andorra"), ("AE", "ARE", "United Arab Emirates"), ("AF", "AFG", "Afghanistan"),
    ("AG", "ATG", "Antigua and Barbuda"), ("AI", "AIA", "Anguilla"), ("AL", "ALB", "Albania"),
    ("AM", "ARM", "Armenia"), ("AO", "AGO", "Angola"), ("AQ", "ATA", "Antarctica"), ("AR", "ARG", "Argentina"),
    ("AS", "ASM", "American Samoa"), ("AT", "AUT", "Austria"), ("AU", "AUS", "Australia"), ("AW", "ABW", "Aruba"),
    ("AX", "ALA", "Åland Islands"), ("AZ", "AZE", "Azerbaijan"), ("BA", "BIH", "Bosnia and Herzegovina"),
    ("BB", "BRB", "Barbados"), ("BD", "BGD", "Bangladesh"), ("BE", "BEL", "Belgium"), ("BF", "BFA", "Burkina Faso"),
    ("BG", "BGR", "Bulgaria"), ("BH", "BHR", "Bahrain"), ("BI", "BDI", "Burundi"), ("BJ", "BEN", "Benin"),
    ("BL", "BLM", "Saint Barthélemy"), ("BM", "BMU", "Bermuda"), ("BN", "BRN", "Brunei"), ("BO", "BOL", "Bolivia"),
    ("BQ", "BES", "Caribbean Netherlands"), ("BR", "BRA", "Brazil"), ("BS", "BHS", "Bahamas"),
    ("BT", "BTN", "Bhutan"), ("BV", "BVT", "Bouvet Island"), ("BW", "BWA", "Botswana"), ("BY", "BLR", "Belarus"),
    ("BZ", "BLZ", "Belize"), ("CA", "CAN", "Canada"), ("CC", "CCK", "Cocos (Keeling) Islands"),
    ("CD", "COD", "Democratic Republic of the Congo"), ("CF", "CAF", "Central African Republic"),
    ("CG", "COG", "Republic of the Congo"), ("CH", "CHE", "Switzerland"), ("CI", "CIV", "Côte d'Ivoire"),
    ("CK", "COK", "Cook Islands"), ("CL", "CHL", "Chile"), ("CM", "CMR", "Cameroon"), ("CN", "CHN", "China"),
    ("CO", "COL", "Colombia"), ("CR", "CRI", "Costa Rica"), ("CU", "CUB", "Cuba"), ("CV", "CPV", "Cape Verde"),
    ("CW", "CUW", "Curaçao"), ("CX", "CXR", "Christmas Island"), ("CY", "CYP", "Cyprus"), ("CZ", "CZE", "Czechia"),
    ("DE", "DEU", "Germany"), ("DJ", "DJI", "Djibouti"), ("DK", "DNK", "Denmark"), ("DM", "DMA", "Dominica"),
    ("DO", "DOM", "Dominican Republic"), ("DZ", "DZA", "Algeria"), ("EC", "ECU", "Ecuador"),
    ("EE", "EST", "Estonia"), ("EG", "EGY", "Egypt"), ("EH", "ESH", "Western Sahara"), ("ER", "ERI", "Eritrea"),
    ("ES", "ESP", "Spain"), ("ET", "ETH", "Ethiopia"), ("FI", "FIN", "Finland"), ("FJ", "FJI", "Fiji"),
    ("FK", "FLK", "Falkland Islands"), ("FM", "FSM", "Micronesia"), ("FO", "FRO", "Faroe Islands"),
    ("FR", "FRA", "France"), ("GA", "GAB", "Gabon"), ("GB", "GBR", "United Kingdom"), ("GD", "GRD", "Grenada"),
    ("GE", "GEO", "Georgia"), ("GF", "GUF", "French Guiana"), ("GG", "GGY", "Guernsey"), ("GH", "GHA", "Ghana"),
    ("GI", "GIB", "Gibraltar"), ("GL", "GRL", "Greenland"), ("GM", "GMB", "Gambia"), ("GN", "GIN", "Guinea"),
    ("GP", "GLP", "Guadeloupe"), ("GQ", "GNQ", "Equatorial Guinea"), ("GR", "GRC", "Greece"),
    ("GS", "SGS", "South Georgia and the South Sandwich Islands"), ("GT", "GTM", "Guatemala"), ("GU", "GUM", "Guam"),
    ("GW", "GNB", "Guinea-Bissau"), ("GY", "GUY", "Guyana"), ("HK", "HKG", "Hong Kong"),
    ("HM", "HMD", "Heard Island and McDonald Islands"), ("HN", "HND", "Honduras"), ("HR", "HRV", "Croatia"),
    ("HT", "HTI", "Haiti"), ("HU", "HUN", "Hungary"), ("ID", "IDN", "Indonesia"), ("IE", "IRL", "Ireland"),
    ("IL", "ISR", "Israel"), ("IM", "IMN", "Isle of Man"), ("IN", "IND", "India"),
    ("IO", "IOT", "British Indian Ocean Territory"), ("IQ", "IRQ", "Iraq"), ("IR", "IRN", "Iran"),
    ("IS", "ISL", "Iceland"), ("IT", "ITA", "Italy"), ("JE", "JEY", "Jersey"), ("JM", "JAM", "Jamaica"),
    ("JO", "JOR", "Jordan"), ("JP", "JPN", "Japan"), ("KE", "KEN", "Kenya"), ("KG", "KGZ", "Kyrgyzstan"),
    ("KH", "KHM", "Cambodia"), ("KI", "KIR", "Kiribati"), ("KM", "COM", "Comoros"),
    ("KN", "KNA", "Saint Kitts and Nevis"), ("KP", "PRK", "North Korea"), ("KR", "KOR", "South Korea"),
    ("KW", "KWT", "Kuwait"), ("KY", "CYM", "Cayman Islands"), ("KZ", "KAZ", "Kazakhstan"), ("LA", "LAO", "Laos"),
    ("LB", "LBN", "Lebanon"), ("LC", "LCA", "Saint Lucia"), ("LI", "LIE", "Liechtenstein"),
    ("LK", "LKA", "Sri Lanka"), ("LR", "LBR", "Liberia"), ("LS", "LSO", "Lesotho"), ("LT", "LTU", "Lithuania"),
    ("LU", "LUX", "Luxembourg"), ("LV", "LVA", "Latvia"), ("LY", "LBY", "Libya"), ("MA", "MAR", "Morocco"),
    ("MC", "MCO", "Monaco"), ("MD", "MDA", "Moldova"), ("ME", "MNE", "Montenegro"), ("MF", "MAF", "Saint Martin"),
    ("MG", "MDG", "Madagascar"), ("MH", "MHL", "Marshall Islands"), ("MK", "MKD", "North Macedonia"),
    ("ML", "MLI", "Mali"), ("MM", "MMR", "Myanmar"), ("MN", "MNG", "Mongolia"), ("MO", "MAC", "Macao"),
    ("MP", "MNP", "Northern Mariana Islands"), ("MQ", "MTQ", "Martinique"), ("MR", "MRT", "Mauritania"),
    ("MS", "MSR", "Montserrat"), ("MT", "MLT", "Malta"), ("MU", "MUS", "Mauritius"), ("MV", "MDV", "Maldives"),
    ("MW", "MWI", "Malawi"), ("MX", "MEX", "Mexico"), ("MY", "MYS", "Malaysia"), ("MZ", "MOZ", "Mozambique"),
    ("NA", "NAM", "Namibia"), ("NC", "NCL", "New Caledonia"), ("NE", "NER", "Niger"),
    ("NF", "NFK", "Norfolk Island"), ("NG", "NGA", "Nigeria"), ("NI", "NIC", "Nicaragua"),
    ("NL", "NLD", "Netherlands"), ("NO", "NOR", "Norway"), ("NP", "NPL", "Nepal"), ("NR", "NRU", "Nauru"),
    ("NU", "NIU", "Niue"), ("NZ", "NZL", "New Zealand"), ("OM", "OMN", "Oman"), ("PA", "PAN", "Panama"),
    ("PE", "PER", "Peru"), ("PF", "PYF", "French Polynesia"), ("PG", "PNG", "Papua New Guinea"),
    ("PH", "PHL", "Philippines"), ("PK", "PAK", "Pakistan"), ("PL", "POL", "Poland"),
    ("PM", "SPM", "Saint Pierre and Miquelon"), ("PN", "PCN", "Pitcairn Islands"), ("PR", "PRI", "Puerto Rico"),
    ("PS", "PSE", "Palestine"), ("PT", "PRT", "Portugal"), ("PW", "PLW", "Palau"), ("PY", "PRY", "Paraguay"),
    ("QA", "QAT", "Qatar"), ("RE", "REU", "Réunion"), ("RO", "ROU", "Romania"), ("RS", "SRB", "Serbia"),
    ("RU", "RUS", "Russia"), ("RW", "RWA", "Rwanda"), ("SA", "SAU", "Saudi Arabia"),
    ("SB", "SLB", "Solomon Islands"), ("SC", "SYC", "Seychelles"), ("SD", "SDN", "Sudan"), ("SE", "SWE", "Sweden"),
    ("SG", "SGP", "Singapore"), ("SH", "SHN", "Saint Helena"), ("SI", "SVN", "Slovenia"),
    ("SJ", "SJM", "Svalbard and Jan Mayen"), ("SK", "SVK", "Slovakia"), ("SL", "SLE", "Sierra Leone"),
    ("SM", "SMR", "San Marino"), ("SN", "SEN", "Senegal"), ("SO", "SOM", "Somalia"), ("SR", "SUR", "Suriname"),
    ("SS", "SSD", "South Sudan"), ("ST", "STP", "São Tomé and Príncipe"), ("SV", "SLV", "El Salvador"),
    ("SX", "SXM", "Sint Maarten"), ("SY", "SYR", "Syria"), ("SZ", "SWZ", "Eswatini"),
    ("TC", "TCA", "Turks and Caicos Islands"), ("TD", "TCD", "Chad"), ("TF", "ATF", "French Southern Territories"),
    ("TG", "TGO", "Togo"), ("TH", "THA", "Thailand"), ("TJ", "TJK", "Tajikistan"), ("TK", "TKL", "Tokelau"),
    ("TL", "TLS", "Timor-Leste"), ("TM", "TKM", "Turkmenistan"), ("TN", "TUN", "Tunisia"), ("TO", "TON", "Tonga"),
    ("TR", "TUR", "Türkiye"), ("TT", "TTO", "Trinidad and Tobago"), ("TV", "TUV", "Tuvalu"), ("TW", "TWN", "Taiwan"),
    ("TZ", "TZA", "Tanzania"), ("UA", "UKR", "Ukraine"), ("UG", "UGA", "Uganda"),
    ("UM", "UMI", "United States Minor Outlying Islands"), ("US", "USA", "United States"), ("UY", "URY", "Uruguay"),
    ("UZ", "UZB", "Uzbekistan"), ("VA", "VAT", "Vatican City"), ("VC", "VCT", "Saint Vincent and the Grenadines"),
    ("VE", "VEN", "Venezuela"), ("VG", "VGB", "British Virgin Islands"), ("VI", "VIR", "U.S. Virgin Islands"),
    ("VN", "VNM", "Vietnam"), ("VU", "VUT", "Vanuatu"), ("WF", "WLF", "Wallis and Futuna"), ("WS", "WSM", "Samoa"),
    ("YE", "YEM", "Yemen"), ("YT", "MYT", "Mayotte"), ("ZA", "ZAF", "South Africa"), ("ZM", "ZMB", "Zambia"),
    ("ZW", "ZWE", "Zimbabwe"),
];

pub(super) const CITIES: &[&str] = &[
    "Tokyo", "Delhi", "Shanghai", "São Paulo", "Mexico City", "Cairo", "Mumbai", "Beijing", "Dhaka", "Osaka",
    "New York", "Karachi", "Buenos Aires", "Istanbul", "Kolkata", "Lagos", "Manila", "Rio de Janeiro", "Guangzhou",
    "Los Angeles", "Moscow", "Kinshasa", "Shenzhen", "Lahore", "Bengaluru", "Paris", "Bogotá", "Jakarta", "Chennai",
    "Lima", "Bangkok", "Seoul", "Hyderabad", "London", "Tehran", "Chicago", "Ho Chi Minh City", "Luanda",
    "Kuala Lumpur", "Hong Kong", "Riyadh", "Santiago", "Pune", "Madrid", "Toronto", "Dallas", "Houston", "Singapore",
    "Barcelona", "Johannesburg", "Saint Petersburg", "Nairobi", "Sydney", "Melbourne", "Berlin", "Rome", "Milan",
    "Casablanca", "Accra", "Addis Ababa", "Dar es Salaam", "Cape Town", "Montreal", "Vancouver", "San Francisco",
    "Seattle", "Boston", "Miami", "Atlanta", "Denver", "Austin", "Hamburg", "Munich", "Frankfurt", "Vienna", "Zürich",
    "Geneva", "Amsterdam", "Rotterdam", "Brussels", "Copenhagen", "Stockholm", "Oslo", "Helsinki", "Dublin",
    "Edinburgh", "Manchester", "Lisbon", "Porto", "Warsaw", "Kraków", "Prague", "Budapest", "Bucharest", "Athens",
    "Kyiv", "Tel Aviv", "Dubai", "Abu Dhabi", "Doha", "Colombo", "Kathmandu", "Auckland", "Wellington", "Perth",
    "Brisbane", "Taipei", "Hanoi", "Medellín", "Quito", "Montevideo", "Guadalajara", "Monterrey", "Havana",
    "San Juan", "Reykjavík", "Tallinn", "Riga", "Vilnius", "Ljubljana", "Zagreb", "Belgrade", "Sofia", "Tbilisi",
    "Almaty", "Tashkent", "Lyon", "Marseille", "Valencia", "Seville", "Naples", "Turin", "Kigali", "Dakar",
    "Abidjan", "Kampala", "Lusaka", "Harare", "Québec City", "Łódź", "Göteborg", "Malmö", "Düsseldorf", "Köln",
];

pub(super) const STREET_BASES: &[&str] = &[
    "Oak", "Maple", "Cedar", "Elm", "Pine", "Willow", "Birch", "Lake", "Hill", "Park", "River", "Church", "Mill",
    "Station", "High", "King", "Queen", "Victoria", "Market", "Bridge", "Garden", "Harbor", "Meadow", "Spring",
    "Sunset", "Forest", "Chestnut", "Rose", "Lincoln", "Washington", "Jefferson", "Madison", "Franklin", "Highland",
    "Valley", "Ridge", "Orchard", "Railway", "School", "Castle", "Abbey", "Canal", "Harbour", "Albert", "Cherry",
    "Magnolia", "Poplar", "Sycamore", "Walnut", "Hawthorn", "Heather", "Lavender", "Juniper", "Aspen", "Ivy",
];
pub(super) const STREET_SUFFIXES: &[&str] = &[
    "Street", "Avenue", "Road", "Lane", "Drive", "Boulevard", "Way", "Court", "Place", "Terrace", "Crescent",
    "Close", "Square", "Parkway", "Row", "Walk", "Circle", "Trail", "Grove", "Gardens",
];

pub(super) const US_STATES: &[(&str, &str)] = &[
    ("Alabama", "AL"), ("Alaska", "AK"), ("Arizona", "AZ"), ("Arkansas", "AR"), ("California", "CA"),
    ("Colorado", "CO"), ("Connecticut", "CT"), ("Delaware", "DE"), ("Florida", "FL"), ("Georgia", "GA"),
    ("Hawaii", "HI"), ("Idaho", "ID"), ("Illinois", "IL"), ("Indiana", "IN"), ("Iowa", "IA"), ("Kansas", "KS"),
    ("Kentucky", "KY"), ("Louisiana", "LA"), ("Maine", "ME"), ("Maryland", "MD"), ("Massachusetts", "MA"),
    ("Michigan", "MI"), ("Minnesota", "MN"), ("Mississippi", "MS"), ("Missouri", "MO"), ("Montana", "MT"),
    ("Nebraska", "NE"), ("Nevada", "NV"), ("New Hampshire", "NH"), ("New Jersey", "NJ"), ("New Mexico", "NM"),
    ("New York", "NY"), ("North Carolina", "NC"), ("North Dakota", "ND"), ("Ohio", "OH"), ("Oklahoma", "OK"),
    ("Oregon", "OR"), ("Pennsylvania", "PA"), ("Rhode Island", "RI"), ("South Carolina", "SC"),
    ("South Dakota", "SD"), ("Tennessee", "TN"), ("Texas", "TX"), ("Utah", "UT"), ("Vermont", "VT"),
    ("Virginia", "VA"), ("Washington", "WA"), ("West Virginia", "WV"), ("Wisconsin", "WI"), ("Wyoming", "WY"),
];

/// ISO 639-1 codes.
pub(super) const LANGUAGE_CODES: &[&str] = &[
    "af", "am", "ar", "az", "be", "bg", "bn", "bs", "ca", "cs", "cy", "da", "de", "el", "en", "eo", "es", "et", "eu",
    "fa", "fi", "fr", "ga", "gl", "gu", "he", "hi", "hr", "hu", "hy", "id", "is", "it", "ja", "ka", "kk", "km", "kn",
    "ko", "ky", "lo", "lt", "lv", "mk", "ml", "mn", "mr", "ms", "my", "nb", "ne", "nl", "no", "ny", "pa", "pl", "ps",
    "pt", "ro", "ru", "si", "sk", "sl", "so", "sq", "sr", "sv", "sw", "ta", "te", "th", "tl", "tr", "uk", "ur", "uz",
    "vi", "xh", "yo", "zh", "zu",
];

/// BCP 47 language tags, as sent in `Accept-Language`.
pub(super) const LANGUAGE_TAGS: &[&str] = &[
    "en-US", "en-GB", "en-AU", "en-CA", "en-IN", "de-DE", "de-AT", "de-CH", "fr-FR", "fr-CA", "fr-BE", "es-ES",
    "es-MX", "es-AR", "pt-BR", "pt-PT", "it-IT", "nl-NL", "nl-BE", "sv-SE", "da-DK", "nb-NO", "fi-FI", "pl-PL",
    "cs-CZ", "hu-HU", "ro-RO", "el-GR", "tr-TR", "ru-RU", "uk-UA", "he-IL", "ar-SA", "ar-EG", "fa-IR", "hi-IN",
    "bn-BD", "ta-IN", "th-TH", "vi-VN", "id-ID", "ms-MY", "fil-PH", "zh-CN", "zh-TW", "zh-HK", "ja-JP", "ko-KR",
    "sw-KE", "af-ZA",
];

/// IANA time zones (canonical names), including half- and quarter-hour offsets.
pub(super) const TIMEZONES: &[&str] = &[
    "UTC", "Africa/Abidjan", "Africa/Cairo", "Africa/Casablanca", "Africa/Johannesburg", "Africa/Lagos",
    "Africa/Nairobi", "Africa/Tunis", "America/Anchorage", "America/Argentina/Buenos_Aires", "America/Bogota",
    "America/Caracas", "America/Chicago", "America/Denver", "America/Edmonton", "America/Halifax", "America/Havana",
    "America/Lima", "America/Los_Angeles", "America/Mexico_City", "America/Montevideo", "America/New_York",
    "America/Panama", "America/Phoenix", "America/Santiago", "America/Sao_Paulo", "America/St_Johns",
    "America/Toronto", "America/Vancouver", "Asia/Almaty", "Asia/Baghdad", "Asia/Bangkok", "Asia/Dhaka", "Asia/Dubai",
    "Asia/Ho_Chi_Minh", "Asia/Hong_Kong", "Asia/Jakarta", "Asia/Jerusalem", "Asia/Karachi", "Asia/Kathmandu",
    "Asia/Kolkata", "Asia/Kuala_Lumpur", "Asia/Manila", "Asia/Riyadh", "Asia/Seoul", "Asia/Shanghai",
    "Asia/Singapore", "Asia/Taipei", "Asia/Tashkent", "Asia/Tehran", "Asia/Tokyo", "Asia/Yangon", "Atlantic/Azores",
    "Atlantic/Reykjavik", "Australia/Adelaide", "Australia/Brisbane", "Australia/Darwin", "Australia/Perth",
    "Australia/Sydney", "Europe/Amsterdam", "Europe/Athens", "Europe/Berlin", "Europe/Brussels", "Europe/Bucharest",
    "Europe/Dublin", "Europe/Helsinki", "Europe/Istanbul", "Europe/Lisbon", "Europe/London", "Europe/Madrid",
    "Europe/Moscow", "Europe/Oslo", "Europe/Paris", "Europe/Prague", "Europe/Rome", "Europe/Stockholm",
    "Europe/Vienna", "Europe/Warsaw", "Europe/Zurich", "Pacific/Auckland", "Pacific/Chatham", "Pacific/Fiji",
    "Pacific/Honolulu",
];

// ── Finance ──────────────────────────────────────────────────────────────────

/// ISO 4217: code, name, symbol.
pub(super) const CURRENCIES: &[(&str, &str, &str)] = &[
    ("USD", "US Dollar", "$"), ("EUR", "Euro", "€"), ("GBP", "Pound Sterling", "£"), ("JPY", "Yen", "¥"),
    ("CNY", "Yuan Renminbi", "¥"), ("INR", "Indian Rupee", "₹"), ("AUD", "Australian Dollar", "A$"),
    ("CAD", "Canadian Dollar", "CA$"), ("CHF", "Swiss Franc", "CHF"), ("SEK", "Swedish Krona", "kr"),
    ("NOK", "Norwegian Krone", "kr"), ("DKK", "Danish Krone", "kr"), ("PLN", "Zloty", "zł"),
    ("CZK", "Czech Koruna", "Kč"), ("HUF", "Forint", "Ft"), ("RON", "Romanian Leu", "lei"),
    ("RSD", "Serbian Dinar", "дин."), ("TRY", "Turkish Lira", "₺"), ("RUB", "Russian Ruble", "₽"),
    ("UAH", "Hryvnia", "₴"), ("ILS", "New Israeli Sheqel", "₪"), ("AED", "UAE Dirham", "د.إ"),
    ("SAR", "Saudi Riyal", "ر.س"), ("QAR", "Qatari Rial", "ر.ق"), ("KWD", "Kuwaiti Dinar", "د.ك"),
    ("BHD", "Bahraini Dinar", ".د.ب"), ("OMR", "Rial Omani", "ر.ع."), ("EGP", "Egyptian Pound", "E£"),
    ("MAD", "Moroccan Dirham", "د.م."), ("NGN", "Naira", "₦"), ("KES", "Kenyan Shilling", "KSh"),
    ("ZAR", "Rand", "R"), ("GHS", "Ghana Cedi", "₵"), ("ETB", "Ethiopian Birr", "Br"),
    ("TZS", "Tanzanian Shilling", "TSh"), ("UGX", "Uganda Shilling", "USh"), ("XOF", "CFA Franc BCEAO", "CFA"),
    ("XAF", "CFA Franc BEAC", "FCFA"), ("BRL", "Brazilian Real", "R$"), ("MXN", "Mexican Peso", "MX$"),
    ("ARS", "Argentine Peso", "$"), ("CLP", "Chilean Peso", "$"), ("COP", "Colombian Peso", "$"), ("PEN", "Sol", "S/"),
    ("UYU", "Peso Uruguayo", "$U"), ("KRW", "Won", "₩"), ("TWD", "New Taiwan Dollar", "NT$"),
    ("HKD", "Hong Kong Dollar", "HK$"), ("SGD", "Singapore Dollar", "S$"), ("MYR", "Malaysian Ringgit", "RM"),
    ("THB", "Baht", "฿"), ("IDR", "Rupiah", "Rp"), ("PHP", "Philippine Peso", "₱"), ("VND", "Dong", "₫"),
    ("PKR", "Pakistan Rupee", "₨"), ("BDT", "Taka", "৳"), ("LKR", "Sri Lanka Rupee", "Rs"),
    ("NPR", "Nepalese Rupee", "Rs"), ("NZD", "New Zealand Dollar", "NZ$"), ("ISK", "Iceland Krona", "kr"),
    ("KZT", "Tenge", "₸"), ("GEL", "Lari", "₾"), ("CRC", "Costa Rican Colon", "₡"), ("DOP", "Dominican Peso", "RD$"),
    ("JMD", "Jamaican Dollar", "J$"), ("XPF", "CFP Franc", "₣"), ("NIO", "Cordoba Oro", "C$"),
    ("GNF", "Guinean Franc", "FG"), ("CDF", "Congolese Franc", "FC"), ("ZMW", "Zambian Kwacha", "ZK"),
];

pub(super) const BANK_ACCOUNT_TYPES: &[&str] = &[
    "Checking", "Savings", "Money Market", "Investment", "Home Loan", "Credit Card", "Auto Loan", "Personal Loan",
];
pub(super) const TRANSACTION_TYPES: &[&str] = &["deposit", "withdrawal", "payment", "invoice", "refund", "transfer"];

/// Card brand, well-known test-number prefixes, length.
pub(super) const CARD_BRANDS: &[(&str, &[&str], usize)] = &[
    ("visa", &["4111", "4242", "4000"], 16), ("mastercard", &["5555", "5105", "2223"], 16),
    ("amex", &["3782", "3714"], 15), ("discover", &["6011"], 16), ("jcb", &["3530", "3566"], 16),
    ("diners", &["3056", "3852"], 14),
];

/// IBAN countries: code, BBAN pattern (`n` digit, `a` capital letter).
/// Belgian, Spanish, French and Dutch account numbers also get their national check.
pub(super) const IBAN_FORMATS: &[(&str, &str)] = &[
    ("AT", "nnnnnnnnnnnnnnnn"), ("BE", "nnnnnnnnnnnn"), ("CH", "nnnnnnnnnnnnnnnnn"), ("DE", "nnnnnnnnnnnnnnnnnn"),
    ("DK", "nnnnnnnnnnnnnn"), ("ES", "nnnnnnnnnnnnnnnnnnnn"), ("FR", "nnnnnnnnnnnnnnnnnnnnnnn"),
    ("GB", "aaaannnnnnnnnnnnnn"), ("IE", "aaaannnnnnnnnnnnnn"), ("NL", "aaaannnnnnnnnn"),
    ("SE", "nnnnnnnnnnnnnnnnnnnn"),
];

// ── Business and commerce ────────────────────────────────────────────────────

pub(super) const COMPANY_SUFFIXES: &[&str] = &[
    "Inc", "LLC", "Group", "Ltd", "GmbH", "AG", "SA", "SAS", "BV", "Pty Ltd", "Co", "KK", "Oy", "AB", "SpA", "PLC",
];
pub(super) const BS_BUZZ: &[&str] = &[
    "implement", "utilize", "integrate", "streamline", "optimize", "evolve", "transform", "embrace", "enable",
    "orchestrate", "leverage", "reinvent", "aggregate", "architect", "enhance", "incentivize", "empower", "monetize",
    "harness", "facilitate", "seize", "synergize", "strategize", "deploy", "brand", "grow", "target", "syndicate",
    "synthesize", "deliver", "mesh", "incubate", "engage", "maximize", "benchmark", "expedite", "visualize",
    "repurpose", "innovate", "scale", "unleash", "drive", "extend", "engineer", "revolutionize", "generate",
    "exploit", "transition", "iterate", "cultivate", "productize", "redefine", "recontextualize",
];
pub(super) const BS_ADJECTIVES: &[&str] = &[
    "clicks-and-mortar", "value-added", "vertical", "proactive", "robust", "revolutionary", "scalable",
    "leading-edge", "innovative", "intuitive", "strategic", "e-business", "mission-critical", "sticky", "one-to-one",
    "24/7", "end-to-end", "global", "B2B", "B2C", "granular", "frictionless", "virtual", "viral", "dynamic", "24/365",
    "best-of-breed", "killer", "magnetic", "bleeding-edge", "web-enabled", "interactive", "real-time", "efficient",
    "front-end", "distributed", "seamless", "extensible", "turn-key", "world-class", "open-source", "cross-platform",
    "synergistic", "out-of-the-box", "enterprise", "integrated", "impactful", "wireless", "transparent",
    "next-generation", "cutting-edge", "user-centric", "visionary", "customized", "ubiquitous", "plug-and-play",
    "collaborative", "compelling", "holistic", "rich",
];
pub(super) const BS_NOUNS: &[&str] = &[
    "synergies", "paradigms", "markets", "partnerships", "infrastructures", "platforms", "initiatives", "channels",
    "eyeballs", "communities", "ROI", "solutions", "e-services", "action-items", "portals", "niches",
    "technologies", "content", "supply-chains", "convergence", "relationships", "architectures", "interfaces",
    "e-markets", "e-commerce", "systems", "bandwidth", "models", "mindshare", "deliverables", "users", "schemas",
    "networks", "applications", "metrics", "functionalities", "experiences", "web services", "methodologies",
    "blockchains", "lifetime value",
];
pub(super) const CATCH_ADJECTIVES: &[&str] = &[
    "Adaptive", "Advanced", "Ameliorated", "Assimilated", "Automated", "Balanced", "Business-focused",
    "Centralized", "Cloned", "Compatible", "Configurable", "Cross-group", "Cross-platform", "Customer-focused",
    "Customizable", "Decentralized", "De-engineered", "Devolved", "Digitized", "Distributed", "Diverse",
    "Down-sized", "Enhanced", "Enterprise-wide", "Ergonomic", "Exclusive", "Expanded", "Extended", "Focused",
    "Front-line", "Fully-configurable", "Function-based", "Fundamental", "Future-proofed", "Grass-roots",
    "Horizontal", "Implemented", "Innovative", "Integrated", "Intuitive", "Inverse", "Managed", "Mandatory",
    "Monitored", "Multi-channelled", "Multi-layered", "Multi-tiered", "Networked", "Object-based", "Open-source",
    "Operative", "Optimized", "Optional", "Organic", "Organized", "Persistent", "Phased", "Pre-emptive",
    "Proactive", "Profit-focused", "Programmable", "Progressive", "Public-key", "Quality-focused", "Reactive",
    "Realigned", "Re-engineered", "Reduced", "Reverse-engineered", "Right-sized", "Robust", "Seamless", "Secured",
    "Self-enabling", "Sharable", "Stand-alone", "Streamlined", "Switchable", "Synchronised", "Synergistic",
    "Team-oriented", "Total", "Triple-buffered", "Universal", "Upgradable", "User-centric", "User-friendly",
    "Versatile", "Virtual", "Visionary",
];
pub(super) const CATCH_DESCRIPTORS: &[&str] = &[
    "24 hour", "24/7", "3rd generation", "4th generation", "5th generation", "actuating", "analyzing", "asymmetric",
    "asynchronous", "attitude-oriented", "background", "bandwidth-monitored", "bi-directional", "bifurcated",
    "bottom-line", "clear-thinking", "client-driven", "client-server", "coherent", "cohesive", "composite",
    "context-sensitive", "contextually-based", "content-based", "dedicated", "demand-driven", "didactic",
    "directional", "discrete", "dynamic", "eco-centric", "empowering", "encompassing", "even-keeled", "executive",
    "explicit", "fault-tolerant", "foreground", "fresh-thinking", "full-range", "global", "grid-enabled",
    "heuristic", "high-level", "holistic", "homogeneous", "hybrid", "incremental", "intangible", "interactive",
    "intermediate", "leading edge", "local", "logistical", "maximized", "methodical", "mission-critical", "mobile",
    "modular", "motivating", "multimedia", "multi-state", "multi-tasking", "national", "needs-based", "neutral",
    "next generation", "non-volatile", "object-oriented", "optimal", "optimizing", "radical", "real-time",
    "reciprocal", "regional", "responsive", "scalable", "secondary", "solution-oriented", "stable", "static",
    "systematic", "systemic", "tangible", "tertiary", "transitional", "uniform", "upward-trending", "user-facing",
    "value-added", "web-enabled", "well-modulated", "zero administration", "zero defect", "zero tolerance",
];
pub(super) const CATCH_NOUNS: &[&str] = &[
    "ability", "access", "adapter", "algorithm", "alliance", "analyzer", "application", "approach", "architecture",
    "archive", "array", "attitude", "benchmark", "capability", "capacity", "challenge", "circuit", "collaboration",
    "complexity", "concept", "conglomeration", "contingency", "core", "customer loyalty", "database",
    "data-warehouse", "definition", "emulation", "encoding", "encryption", "extranet", "firmware", "flexibility",
    "focus group", "forecast", "frame", "framework", "function", "groupware", "hardware", "help-desk", "hierarchy",
    "hub", "implementation", "infrastructure", "initiative", "installation", "instruction set", "interface",
    "intranet", "knowledge base", "leverage", "matrix", "methodology", "middleware", "migration", "model",
    "moderator", "monitoring", "moratorium", "neural-net", "open architecture", "orchestration", "paradigm",
    "parallelism", "policy", "portal", "pricing structure", "process improvement", "product", "productivity",
    "project", "projection", "protocol", "secured line", "service-desk", "software", "solution", "standardization",
    "strategy", "structure", "success", "superstructure", "support", "synergy", "system engine", "task-force",
    "throughput", "time-frame", "toolset", "website", "workforce",
];

pub(super) const PRODUCT_ADJECTIVES: &[&str] = &[
    "Small", "Ergonomic", "Rustic", "Intelligent", "Gorgeous", "Incredible", "Fantastic", "Practical", "Sleek",
    "Awesome", "Generic", "Handcrafted", "Handmade", "Licensed", "Refined", "Unbranded", "Tasty", "Modern",
    "Recycled", "Electronic", "Luxurious", "Bespoke", "Elegant", "Compact", "Portable", "Premium",
];
pub(super) const PRODUCT_MATERIALS: &[&str] = &[
    "Steel", "Wooden", "Concrete", "Plastic", "Cotton", "Granite", "Rubber", "Metal", "Soft", "Fresh", "Frozen",
    "Bronze", "Ceramic", "Leather", "Linen", "Marble", "Silk", "Wool", "Bamboo", "Glass", "Aluminum", "Titanium",
];
pub(super) const PRODUCTS: &[&str] = &[
    "Chair", "Car", "Computer", "Keyboard", "Mouse", "Bike", "Ball", "Gloves", "Pants", "Shirt", "Table", "Shoes",
    "Hat", "Towels", "Soap", "Tuna", "Chicken", "Fish", "Cheese", "Bacon", "Pizza", "Salad", "Sausages", "Chips",
    "Laptop", "Monitor", "Headphones", "Backpack", "Watch", "Lamp", "Sofa", "Mug", "Bottle", "Phone", "Tablet",
    "Camera", "Jacket", "Sneakers", "Umbrella", "Wallet", "Blanket", "Pillow", "Speaker", "Charger",
];
pub(super) const DEPARTMENTS: &[&str] = &[
    "Books", "Movies", "Music", "Games", "Electronics", "Computers", "Home", "Garden", "Tools", "Grocery", "Health",
    "Beauty", "Toys", "Kids", "Baby", "Clothing", "Shoes", "Jewelry", "Sports", "Outdoors", "Automotive",
    "Industrial", "Pets", "Office",
];

// ── Words ────────────────────────────────────────────────────────────────────

pub(super) const ABBREVIATIONS: &[&str] = &[
    "ADP", "AGP", "AI", "API", "ASCII", "CLI", "CORS", "CPU", "CSS", "CSV", "DNS", "EXE", "FTP", "GPU", "GUI", "HDD",
    "HTTP", "HTTPS", "IP", "JSON", "JWT", "OCR", "PCI", "PNG", "RAM", "REST", "RSS", "SAS", "SCSI", "SDK", "SMS",
    "SMTP", "SQL", "SSD", "SSH", "SSL", "TCP", "TLS", "UDP", "URL", "USB", "UTF8", "UUID", "XML", "YAML",
];
pub(super) const ADJECTIVES: &[&str] = &[
    "auxiliary", "primary", "back-end", "digital", "open-source", "virtual", "cross-platform", "redundant", "online",
    "haptic", "multi-byte", "bluetooth", "wireless", "1080p", "neural", "optical", "solid state", "mobile",
    "headless", "distributed", "asynchronous", "stateless", "idempotent", "cached", "encrypted", "legacy", "remote",
];
pub(super) const NOUNS: &[&str] = &[
    "driver", "protocol", "bandwidth", "panel", "microchip", "program", "port", "card", "array", "interface",
    "system", "sensor", "firewall", "hard drive", "pixel", "alarm", "feed", "monitor", "application", "transmitter",
    "bus", "circuit", "capacitor", "matrix", "endpoint", "payload", "cluster", "container", "pipeline", "socket",
    "queue", "token", "schema", "cache", "proxy", "certificate", "webhook", "index",
];
pub(super) const VERBS: &[&str] = &[
    "back up", "bypass", "hack", "override", "compress", "copy", "navigate", "index", "connect", "generate",
    "quantify", "calculate", "synthesize", "input", "transmit", "program", "reboot", "parse", "deploy", "refactor",
    "serialize", "cache", "encrypt", "validate", "migrate", "throttle",
];
pub(super) const ING_VERBS: &[&str] = &[
    "backing up", "bypassing", "hacking", "overriding", "compressing", "copying", "navigating", "indexing",
    "connecting", "generating", "quantifying", "calculating", "synthesizing", "transmitting", "programming",
    "parsing", "deploying", "refactoring", "serializing", "caching", "encrypting", "validating", "migrating",
    "throttling",
];
/// `{a}` abbreviation, `{j}` adjective, `{n}` noun, `{v}` verb, `{g}` -ing verb.
pub(super) const PHRASES: &[&str] = &[
    "If we {v} the {n}, we can get to the {a} {n} through the {j} {a} {n}!",
    "We need to {v} the {j} {a} {n}!",
    "Try to {v} the {a} {n}, maybe it will {v} the {j} {n}!",
    "You can't {v} the {n} without {g} the {j} {a} {n}!",
    "Use the {j} {a} {n}, then you can {v} the {j} {n}!",
    "The {a} {n} is down, {v} the {j} {n} so we can {v} the {a} {n}!",
    "{G} the {n} won't do anything, we need to {v} the {j} {a} {n}!",
    "I'll {v} the {j} {a} {n}, that should {v} the {a} {n}!",
];

pub(super) const LOREM_WORDS: &[&str] = &[
    "lorem", "ipsum", "dolor", "sit", "amet", "consectetur", "adipiscing", "elit", "sed", "do", "eiusmod", "tempor",
    "incididunt", "ut", "labore", "et", "dolore", "magna", "aliqua", "enim", "ad", "minim", "veniam", "quis",
    "nostrud", "exercitation", "ullamco", "laboris", "nisi", "aliquip", "ex", "ea", "commodo", "consequat", "duis",
    "aute", "irure", "in", "reprehenderit", "voluptate", "velit", "esse", "cillum", "fugiat", "nulla", "pariatur",
    "excepteur", "sint", "occaecat", "cupidatat", "non", "proident", "sunt", "culpa", "qui", "officia", "deserunt",
    "mollit", "anim", "id", "est", "laborum", "accusamus", "accusantium", "alias", "aliquam", "aperiam",
    "architecto", "asperiores", "aspernatur", "assumenda", "at", "atque", "autem", "beatae", "blanditiis",
    "commodi", "consequatur", "consequuntur", "corporis", "corrupti", "cum", "cumque", "debitis", "delectus",
    "deleniti", "dicta", "dignissimos", "distinctio", "doloremque", "dolores", "doloribus", "dolorum", "ducimus",
    "earum", "eius", "eligendi", "eos", "error", "eum", "eveniet", "exercitationem", "expedita", "explicabo",
    "facere", "facilis", "harum", "hic", "iste", "iure", "iusto", "laboriosam", "laudantium", "libero", "magnam",
    "magni", "maiores", "maxime", "minima", "minus", "modi", "molestiae", "molestias", "natus", "necessitatibus",
    "nemo", "neque", "nesciunt", "nihil", "nobis", "numquam", "obcaecati", "odio", "odit", "omnis", "optio",
    "perferendis", "perspiciatis", "placeat", "porro", "possimus", "praesentium", "provident", "quae", "quaerat",
    "quam", "quas", "quasi", "quia", "quibusdam", "quidem", "quisquam", "quo", "quod", "quos", "ratione",
    "recusandae", "reiciendis", "rem", "repellat", "repellendus", "repudiandae", "rerum", "saepe", "sapiente",
    "similique", "soluta", "tempora", "tempore", "temporibus", "tenetur", "totam", "ullam", "unde", "vel",
    "veritatis", "vero", "vitae", "voluptas", "voluptatem", "voluptates", "voluptatibus", "voluptatum",
];

// ── Text ─────────────────────────────────────────────────────────────────────

/// Emoji, including ones made of several code points (skin tones, ZWJ sequences, flags).
pub(super) const EMOJI: &[&str] = &[
    "😀", "😂", "🥲", "😍", "🤔", "😎", "🙃", "😴", "🤯", "🥳", "😭", "😡", "👍", "👎", "👏", "🙏", "💪", "👋🏽",
    "👩‍💻", "🧑‍🚀", "👨‍👩‍👧", "❤️", "💔", "🔥", "✨", "🎉", "🚀", "⭐", "🌈", "☀️", "❄️", "🍕", "🍣", "☕", "🐶",
    "🐱", "🦄", "🐙", "🌍", "🏠", "🚗", "✈️", "📱", "💻", "📦", "🔒", "✅", "❌", "⚠️", "💡", "🇺🇸", "🇯🇵", "🇧🇷",
    "🇮🇳", "🇩🇪", "🏳️‍🌈", "1️⃣",
];

/// Character pools for `$randomUnicodeString`: accented Latin, Greek and Cyrillic,
/// CJK and Hangul, right-to-left (Arabic, Hebrew), Indic and Thai, emoji.
pub(super) const UNICODE_POOLS: &[&str] = &[
    "àáâãäåæçèéêëìíîïñòóôõöøùúûüýÿßœšžłćńśźőűĀĒĪŌŪ",
    "αβγδεζηθλμπσφωΩЖДЛПФЦЧШЩЫЯжщю",
    "漢字日本語中文東京北京你好世界こんにちはカタカナ한국어서울",
    "مرحباالعربيةشكراשלוםעבריתתודה",
    "नमस्तेहिन्दीகணினிสวัสดีไทย",
    "😀🚀🎉🔥🌍💡✅🍕🐱🦄",
];

// ── Internet ─────────────────────────────────────────────────────────────────

pub(super) const DOMAIN_SUFFIXES: &[&str] = &["com", "net", "org", "io", "dev", "app", "info", "biz", "co", "name"];
/// Reserved for documentation (RFC 2606): mail to them never reaches anyone.
pub(super) const EMAIL_DOMAINS: &[&str] = &["example.com", "example.net", "example.org"];
pub(super) const HTTP_METHODS: &[&str] = &["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];
pub(super) const HTTP_STATUSES: &[u16] = &[
    200, 201, 202, 204, 206, 301, 302, 303, 304, 307, 308, 400, 401, 403, 404, 405, 406, 409, 410, 412, 413, 415,
    418, 422, 425, 428, 429, 431, 500, 501, 502, 503, 504,
];
pub(super) const USER_AGENTS: &[&str] = &[
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36",
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36",
    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/139.0.0.0 Safari/537.36",
    "Mozilla/5.0 (Linux; Android 10; K) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Mobile Safari/537.36",
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36 Edg/140.0.0.0",
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/139.0.0.0 Safari/537.36 OPR/122.0.0.0",
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:143.0) Gecko/20100101 Firefox/143.0",
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10.15; rv:143.0) Gecko/20100101 Firefox/143.0",
    "Mozilla/5.0 (X11; Ubuntu; Linux x86_64; rv:142.0) Gecko/20100101 Firefox/142.0",
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/26.0 Safari/605.1.15",
    "Mozilla/5.0 (iPhone; CPU iPhone OS 18_6 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/26.0 Mobile/15E148 Safari/604.1",
    "Mozilla/5.0 (iPad; CPU OS 18_6 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/26.0 Mobile/15E148 Safari/604.1",
    "Mozilla/5.0 (Linux; Android 14; SAMSUNG SM-S921B) AppleWebKit/537.36 (KHTML, like Gecko) SamsungBrowser/28.0 Chrome/130.0.0.0 Mobile Safari/537.36",
];

// ── Files and databases ──────────────────────────────────────────────────────

/// MIME type, extension, common (a type most APIs and people meet every day).
pub(super) const MIME_TYPES: &[(&str, &str, bool)] = &[
    ("application/pdf", "pdf", true), ("application/json", "json", true), ("application/zip", "zip", true),
    ("application/xml", "xml", true), ("application/msword", "doc", true),
    ("application/vnd.openxmlformats-officedocument.wordprocessingml.document", "docx", true),
    ("application/vnd.openxmlformats-officedocument.spreadsheetml.sheet", "xlsx", true),
    ("text/plain", "txt", true), ("text/csv", "csv", true), ("text/html", "html", true), ("text/css", "css", true),
    ("text/javascript", "js", true), ("image/png", "png", true), ("image/jpeg", "jpg", true),
    ("image/gif", "gif", true), ("image/webp", "webp", true), ("image/svg+xml", "svg", true),
    ("audio/mpeg", "mp3", true), ("audio/wav", "wav", true), ("video/mp4", "mp4", true), ("video/mpeg", "mpeg", true),
    ("video/webm", "webm", true), ("application/gzip", "gz", false), ("application/x-tar", "tar", false),
    ("application/x-7z-compressed", "7z", false), ("application/vnd.rar", "rar", false),
    ("application/octet-stream", "bin", false), ("application/java-archive", "jar", false),
    ("application/epub+zip", "epub", false), ("application/rtf", "rtf", false),
    ("application/vnd.ms-excel", "xls", false), ("application/vnd.ms-powerpoint", "ppt", false),
    ("application/vnd.openxmlformats-officedocument.presentationml.presentation", "pptx", false),
    ("application/vnd.oasis.opendocument.text", "odt", false),
    ("application/vnd.oasis.opendocument.spreadsheet", "ods", false), ("application/x-sh", "sh", false),
    ("application/wasm", "wasm", false), ("application/yaml", "yaml", false), ("application/sql", "sql", false),
    ("application/ld+json", "jsonld", false), ("application/geo+json", "geojson", false),
    ("application/vnd.sqlite3", "sqlite", false), ("application/vnd.apple.mpegurl", "m3u8", false),
    ("application/pkcs12", "p12", false), ("application/x-x509-ca-cert", "crt", false),
    ("application/vnd.android.package-archive", "apk", false), ("application/x-apple-diskimage", "dmg", false),
    ("text/markdown", "md", false), ("text/calendar", "ics", false), ("text/vcard", "vcf", false),
    ("text/tab-separated-values", "tsv", false), ("image/avif", "avif", false), ("image/bmp", "bmp", false),
    ("image/tiff", "tiff", false), ("image/x-icon", "ico", false), ("image/heic", "heic", false),
    ("audio/ogg", "ogg", false), ("audio/flac", "flac", false), ("audio/aac", "aac", false),
    ("audio/midi", "mid", false), ("video/quicktime", "mov", false), ("video/x-msvideo", "avi", false),
    ("video/x-matroska", "mkv", false), ("video/ogg", "ogv", false), ("video/3gpp", "3gp", false),
    ("font/woff", "woff", false), ("font/woff2", "woff2", false), ("font/ttf", "ttf", false),
    ("font/otf", "otf", false), ("model/gltf+json", "gltf", false), ("model/gltf-binary", "glb", false),
    ("model/stl", "stl", false), ("model/obj", "obj", false),
];
pub(super) const DIRECTORIES: &[&str] = &[
    "/usr/bin", "/usr/local/bin", "/usr/lib", "/usr/share", "/usr/local/share", "/etc", "/etc/nginx", "/etc/ssl",
    "/var/log", "/var/lib", "/var/www/html", "/var/cache", "/opt", "/opt/app", "/home/user", "/home/user/Documents",
    "/home/user/Downloads", "/root", "/tmp", "/srv", "/srv/data", "/mnt/data", "/boot", "/lib", "/sbin",
];

pub(super) const DB_COLUMNS: &[&str] = &[
    "id", "title", "name", "email", "phone", "token", "group", "category", "password", "comment", "avatar", "status",
    "createdAt", "updatedAt", "deletedAt", "userId", "tenantId", "slug", "description", "price", "quantity",
];
pub(super) const DB_TYPES: &[&str] = &[
    "int", "varchar", "text", "date", "datetime", "tinyint", "time", "timestamp", "smallint", "mediumint", "bigint",
    "decimal", "float", "double", "real", "bit", "boolean", "serial", "blob", "binary", "enum", "set", "geometry",
    "point", "json", "jsonb", "uuid",
];
pub(super) const DB_COLLATIONS: &[&str] = &[
    "utf8_unicode_ci", "utf8_general_ci", "utf8_bin", "ascii_bin", "ascii_general_ci", "cp1250_bin",
    "cp1250_general_ci", "latin1_swedish_ci", "utf8mb4_unicode_ci", "utf8mb4_general_ci", "utf8mb4_0900_ai_ci",
    "utf8mb4_bin",
];
pub(super) const DB_ENGINES: &[&str] = &["InnoDB", "MyISAM", "MEMORY", "CSV", "BLACKHOLE", "ARCHIVE"];

// ── Colors and dates ─────────────────────────────────────────────────────────

pub(super) const COLOR_NAMES: &[&str] = &[
    "red", "green", "blue", "yellow", "purple", "mint green", "teal", "white", "black", "orange", "pink", "grey",
    "maroon", "violet", "turquoise", "tan", "sky blue", "salmon", "plum", "orchid", "olive", "magenta", "lime",
    "ivory", "indigo", "gold", "fuchsia", "cyan", "azure", "lavender", "silver",
];
pub(super) const WEEKDAYS: &[&str] = &["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];
pub(super) const MONTHS: &[&str] = &[
    "January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November",
    "December",
];
