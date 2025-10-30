from babel.messages import mofile, pofile

with open('locale/es/LC_MESSAGES/launchtype.po', 'rb') as f:
    catalog = pofile.read_po(f)

with open('locale/es/LC_MESSAGES/launchtype.mo', 'wb') as f:
    mofile.write_mo(f, catalog)

print("Translation compiled successfully!")
